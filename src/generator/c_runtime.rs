//! The Cyclone runtime, carried verbatim into `runtime.h`.
//!
//! The C counterpart of [`super::cpp_runtime`] - same wire format, same
//! byte-by-byte discipline - but reshaped for a language with no classes, no
//! exceptions and no growable container types of its own.
//!
//! # Free functions, not methods
//!
//! `CycloneWriter` and `CycloneReader` are plain `struct`s; every operation on
//! one is a `static inline` free function taking a pointer to it as its first
//! argument (`cyclone_writer_write_u32(writer, value)`, not
//! `writer.write_u32(value)`). `static inline` is what makes this
//! header-only and safe: a function defined that way in a header gets its own
//! private copy in every translation unit that includes it, so there is
//! nothing to link and no risk of "multiple definition" the way a plain
//! `void foo(void) { ... }` in a header would risk the moment two `.c` files
//! included it.
//!
//! # `bool` where C++ needed nothing at all
//!
//! [`super::cpp_runtime`]'s `Writer` cannot fail: `std::vector` throws on
//! allocation failure, which - in the no-exceptions C++ this generator
//! targets - is either turned off entirely (`-fno-exceptions`, `terminate()`
//! on failure) or left to unwind past code that was not written to catch it.
//! Plain C has neither escape hatch: `malloc`/`realloc` return `NULL` and the
//! rest is silence. So every `cyclone_writer_write_*` function here returns
//! `bool` - `true` for success, `false` for "out of memory" - and every
//! generated `encode` propagates that the same way a generated `decode`
//! propagates a [`CycloneDecodeError`]: `if (!cyclone_writer_write_u32(...))
//! return false;`.
//!
//! # Ownership: decode allocates, `_free` releases
//!
//! A `string` field decodes into a heap-allocated, NUL-terminated `char*`; a
//! `bytes` field decodes into a [`CycloneBytes`] (`{ data, len }`), also
//! heap-allocated; an `Array<T>` field decodes into a generated
//! `CycloneArray_T` (see `arrays.h`, written per schema because its shape
//! depends on `T`). Every one of those is `malloc`'d by decode and must be
//! released exactly once, by calling the model's generated `<Model>_free` -
//! never by hand, and never by assuming a plain `free()` of the struct itself
//! reaches what its pointers point to.
//!
//! **A decoded value must not be decoded into again without being freed
//! first.** Nothing here calls `free` on whatever a field already held before
//! writing a new pointer over it - doing so unconditionally would be its own
//! bug (freeing a pointer the caller never actually owned, if the struct was
//! merely freshly zeroed and never decoded before). Always decode into a
//! struct that is either freshly zero-initialized (`Player value = {0};`) or
//! has just been passed to `Player_free`.
//!
//! # Endianness is never assumed
//!
//! Identical policy to every other backend: every multi-byte value is
//! assembled and disassembled byte by byte with shifts and masks, never
//! `memcpy`d as a whole multi-byte value and never read through a
//! reinterpreted pointer. `memcpy` appears only to reinterpret an IEEE-754
//! `float`/`double`'s bits as an integer of the same width.

/// The runtime block, emitted once into `runtime.h`.
pub const RUNTIME: &str = r####"
/* ==========================================================================
 * Cyclone runtime - RFC-0002, carried verbatim.
 *
 * Not generated from your models: this block is identical in every project
 * cyclonec generates for. It is here so the generated tree is self-contained
 * - nothing to add to your build beyond this directory, nothing to link.
 * ========================================================================== */

/* A byte stream that does not satisfy the Cyclone Specification.
 *
 * `CYCLONE_DECODE_OK` is `0`, so a zero-initialized `CycloneDecodeError`
 * (`CycloneDecodeError error = {0};`) already means "no error" - the same
 * value every generated `decode` returns on success, and the value a fresh
 * local already starts as. */
typedef enum CycloneDecodeErrorKind {
    CYCLONE_DECODE_OK = 0,
    /* Fewer bytes remain than the value being read requires, **after the
     * read had already begun**.
     *
     * Bytes running out exactly on a field boundary is not this error - it
     * is version skew (RFC-0002 SS9.1), and the generated decoder handles it
     * without asking the runtime. */
    CYCLONE_DECODE_UNEXPECTED_EOF,
    /* A `bool` byte that is neither `0x00` nor `0x01` (RFC-0002 SS2.4). */
    CYCLONE_DECODE_INVALID_BOOL,
    /* A `string` region that is not valid UTF-8. */
    CYCLONE_DECODE_INVALID_UTF8,
    /* A length field beyond the configured limit. */
    CYCLONE_DECODE_LENGTH_OVERFLOW,
    /* `malloc`/`realloc` returned `NULL` while decoding a `string`, `bytes`
     * or `Array<T>` field - the one error kind with no C++ counterpart,
     * since `std::string`/`std::vector` do not report allocation failure
     * this way. */
    CYCLONE_DECODE_OUT_OF_MEMORY,
} CycloneDecodeErrorKind;

typedef struct CycloneDecodeError {
    CycloneDecodeErrorKind kind;
    /* CYCLONE_DECODE_UNEXPECTED_EOF: bytes the read needed. */
    size_t needed;
    /* CYCLONE_DECODE_UNEXPECTED_EOF: bytes actually left. */
    size_t remaining;
    /* CYCLONE_DECODE_INVALID_BOOL: the byte that was neither 0x00 nor 0x01. */
    unsigned char invalid_byte;
    /* CYCLONE_DECODE_LENGTH_OVERFLOW: the length read from the stream. */
    size_t length;
    /* CYCLONE_DECODE_LENGTH_OVERFLOW: the limit it exceeded. */
    size_t limit;
} CycloneDecodeError;

/* Whether `error` is "no error" - the value a zero-initialized
 * `CycloneDecodeError` already has, and the value every generated `decode`
 * returns on success. */
static inline bool cyclone_decode_error_ok(const CycloneDecodeError *error) {
    return error->kind == CYCLONE_DECODE_OK;
}

/* A human-readable description, for logging - never parsed back into a
 * `CycloneDecodeError`, so its wording is not part of any contract. Writes
 * at most `size` bytes into `buffer`, including the terminating NUL. */
static inline void cyclone_decode_error_message(const CycloneDecodeError *error, char *buffer,
                                                  size_t size) {
    switch (error->kind) {
        case CYCLONE_DECODE_OK:
            snprintf(buffer, size, "no error");
            return;
        case CYCLONE_DECODE_UNEXPECTED_EOF:
            snprintf(buffer, size, "unexpected eof: needed %zu bytes, %zu remaining",
                      error->needed, error->remaining);
            return;
        case CYCLONE_DECODE_INVALID_BOOL:
            snprintf(buffer, size, "invalid bool: 0x%02X is neither 0x00 nor 0x01",
                      (unsigned)error->invalid_byte);
            return;
        case CYCLONE_DECODE_INVALID_UTF8:
            snprintf(buffer, size, "invalid utf-8 in string");
            return;
        case CYCLONE_DECODE_LENGTH_OVERFLOW:
            snprintf(buffer, size, "length overflow: length %zu exceeds limit %zu",
                      error->length, error->limit);
            return;
        case CYCLONE_DECODE_OUT_OF_MEMORY:
            snprintf(buffer, size, "out of memory");
            return;
    }
    snprintf(buffer, size, "unknown decode error");
}

/* Allocation guards applied while decoding (RFC-0002 SS12).
 *
 * A `u32` length can claim up to 4 GiB, so a decoder that allocates straight
 * from an untrusted one is a denial-of-service target. These are **not part
 * of the wire format**: two peers with different limits may disagree about a
 * byte stream, and neither is wrong. */
typedef struct CycloneLimits {
    /* Largest accepted UTF-8 byte length of a `string`. */
    size_t max_string_len;
    /* Largest accepted byte length of a `bytes` blob. */
    size_t max_bytes_len;
    /* Largest accepted element count of an `Array<T>` (RFC-0002 SS6). */
    size_t max_array_count;
} CycloneLimits;

/* The permissive default: every field's own default is already `0xFFFFFFFF`
 * - `u32`'s widest value, the widest length RFC-0002 can even express. */
static inline CycloneLimits cyclone_limits_unlimited(void) {
    CycloneLimits limits;
    limits.max_string_len = (size_t)0xFFFFFFFFu;
    limits.max_bytes_len = (size_t)0xFFFFFFFFu;
    limits.max_array_count = (size_t)0xFFFFFFFFu;
    return limits;
}

/* An owned, heap-allocated `bytes` blob - a `malloc`'d buffer and its
 * length. `data` is `NULL` exactly when `len` is `0`. */
typedef struct CycloneBytes {
    unsigned char *data;
    size_t len;
} CycloneBytes;

/* Releases the buffer a decoded `CycloneBytes` owns and zeroes it, so freeing
 * it twice is always safe (freeing `NULL` is a no-op, per the C standard). */
static inline void cyclone_bytes_free(CycloneBytes *bytes) {
    free(bytes->data);
    bytes->data = NULL;
    bytes->len = 0;
}

/* Appends Cyclone-encoded values to a growable, `malloc`-backed buffer.
 *
 * Every multi-byte value is Little Endian, with no padding, no alignment and
 * no metadata between values. Every `cyclone_writer_write_*` function
 * returns `false` if growing the buffer failed - the buffer is left exactly
 * as it was before the call, so a caller may retry after freeing memory
 * elsewhere, or simply propagate the failure, as generated `encode`
 * functions do. */
typedef struct CycloneWriter {
    unsigned char *data;
    size_t len;
    size_t cap;
} CycloneWriter;

/* Initializes an empty writer. Always call this (or `= {0}`) before using
 * one - an uninitialized `CycloneWriter` is not a valid one. */
static inline void cyclone_writer_init(CycloneWriter *writer) {
    writer->data = NULL;
    writer->len = 0;
    writer->cap = 0;
}

/* Releases the writer's buffer and zeroes it, so freeing it twice - or
 * freeing it after `cyclone_writer_take` - is always safe. */
static inline void cyclone_writer_free(CycloneWriter *writer) {
    free(writer->data);
    writer->data = NULL;
    writer->len = 0;
    writer->cap = 0;
}

/* Hands the writer's buffer to the caller, who now owns it and must
 * eventually `free()` it. `*out_len` is set to the number of bytes written.
 * The writer is left empty, so a later `cyclone_writer_free` on it is a
 * no-op. */
static inline unsigned char *cyclone_writer_take(CycloneWriter *writer, size_t *out_len) {
    unsigned char *data = writer->data;
    *out_len = writer->len;
    writer->data = NULL;
    writer->len = 0;
    writer->cap = 0;
    return data;
}

/* Ensures room for `additional` more bytes, growing (by doubling) if not.
 * Returns `false`, buffer untouched, if `realloc` fails. */
static inline bool cyclone_writer_reserve(CycloneWriter *writer, size_t additional) {
    if (writer->cap - writer->len >= additional) {
        return true;
    }
    size_t needed = writer->len + additional;
    size_t new_cap = writer->cap == 0 ? 64 : writer->cap;
    while (new_cap < needed) {
        new_cap *= 2;
    }
    unsigned char *grown = (unsigned char *)realloc(writer->data, new_cap);
    if (grown == NULL) {
        return false;
    }
    writer->data = grown;
    writer->cap = new_cap;
    return true;
}

static inline bool cyclone_writer_push(CycloneWriter *writer, unsigned char byte) {
    if (!cyclone_writer_reserve(writer, 1)) {
        return false;
    }
    writer->data[writer->len++] = byte;
    return true;
}

static inline bool cyclone_writer_write_le(CycloneWriter *writer, uint64_t value,
                                            size_t byte_count) {
    if (!cyclone_writer_reserve(writer, byte_count)) {
        return false;
    }
    for (size_t i = 0; i < byte_count; ++i) {
        writer->data[writer->len++] = (unsigned char)(value >> (8 * i));
    }
    return true;
}

/* Writes a `bool` as one byte: `0x00` or `0x01`, never anything else. */
static inline bool cyclone_writer_write_bool(CycloneWriter *writer, bool value) {
    return cyclone_writer_push(writer, value ? (unsigned char)1 : (unsigned char)0);
}

/* Writes an `i8` as 1 byte. */
static inline bool cyclone_writer_write_i8(CycloneWriter *writer, int8_t value) {
    return cyclone_writer_push(writer, (unsigned char)value);
}

/* Writes a `u8` as 1 byte. */
static inline bool cyclone_writer_write_u8(CycloneWriter *writer, uint8_t value) {
    return cyclone_writer_push(writer, (unsigned char)value);
}

/* Writes an `i16` as 2 bytes, Little Endian. */
static inline bool cyclone_writer_write_i16(CycloneWriter *writer, int16_t value) {
    return cyclone_writer_write_le(writer, (uint16_t)value, 2);
}

/* Writes a `u16` as 2 bytes, Little Endian. */
static inline bool cyclone_writer_write_u16(CycloneWriter *writer, uint16_t value) {
    return cyclone_writer_write_le(writer, value, 2);
}

/* Writes an `i32` as 4 bytes, Little Endian. */
static inline bool cyclone_writer_write_i32(CycloneWriter *writer, int32_t value) {
    return cyclone_writer_write_le(writer, (uint32_t)value, 4);
}

/* Writes a `u32` as 4 bytes, Little Endian. */
static inline bool cyclone_writer_write_u32(CycloneWriter *writer, uint32_t value) {
    return cyclone_writer_write_le(writer, value, 4);
}

/* Writes an `i64` as 8 bytes, Little Endian. */
static inline bool cyclone_writer_write_i64(CycloneWriter *writer, int64_t value) {
    return cyclone_writer_write_le(writer, (uint64_t)value, 8);
}

/* Writes a `u64` as 8 bytes, Little Endian. */
static inline bool cyclone_writer_write_u64(CycloneWriter *writer, uint64_t value) {
    return cyclone_writer_write_le(writer, value, 8);
}

/* Writes an `f32` as its raw IEEE 754 bits, 4 bytes Little Endian. The bit
 * pattern is written unmodified: `NaN` payloads survive and `-0.0` stays
 * distinct from `0.0`. */
static inline bool cyclone_writer_write_f32(CycloneWriter *writer, float value) {
    uint32_t bits;
    memcpy(&bits, &value, sizeof(bits));
    return cyclone_writer_write_u32(writer, bits);
}

/* Writes an `f64` as its raw IEEE 754 bits, 8 bytes Little Endian. */
static inline bool cyclone_writer_write_f64(CycloneWriter *writer, double value) {
    uint64_t bits;
    memcpy(&bits, &value, sizeof(bits));
    return cyclone_writer_write_u64(writer, bits);
}

/* Writes a `string` as a `u32` UTF-8 **byte** length, then those bytes.
 * `value` may be `NULL`, written the same as an empty string - the zero
 * value of a field this codec never decoded. */
static inline bool cyclone_writer_write_string(CycloneWriter *writer, const char *value) {
    size_t len = value != NULL ? strlen(value) : 0;
    if (len > 0xFFFFFFFFu) {
        return false;
    }
    if (!cyclone_writer_write_u32(writer, (uint32_t)len)) {
        return false;
    }
    if (len == 0) {
        return true;
    }
    if (!cyclone_writer_reserve(writer, len)) {
        return false;
    }
    memcpy(writer->data + writer->len, value, len);
    writer->len += len;
    return true;
}

/* Writes a `bytes` blob as a `u32` length, then the raw bytes. */
static inline bool cyclone_writer_write_bytes(CycloneWriter *writer, const CycloneBytes *value) {
    if (value->len > 0xFFFFFFFFu) {
        return false;
    }
    if (!cyclone_writer_write_u32(writer, (uint32_t)value->len)) {
        return false;
    }
    if (value->len == 0) {
        return true;
    }
    if (!cyclone_writer_reserve(writer, value->len)) {
        return false;
    }
    memcpy(writer->data + writer->len, value->data, value->len);
    writer->len += value->len;
    return true;
}

/* Writes an `Array<T>`'s element count (RFC-0002 SS6) - the caller writes
 * each element itself, in order, right after. */
static inline bool cyclone_writer_write_array_count(CycloneWriter *writer, size_t count) {
    if (count > 0xFFFFFFFFu) {
        return false;
    }
    return cyclone_writer_write_u32(writer, (uint32_t)count);
}

/* Reads Cyclone-encoded values from a borrowed buffer.
 *
 * Malformed input is always a `CycloneDecodeError`, never a crash and never a
 * silent wrong answer, and a failed read leaves the cursor where it was. The
 * buffer is borrowed - a `CycloneReader` never outlives the bytes it was
 * initialized over. */
typedef struct CycloneReader {
    const unsigned char *data;
    size_t size;
    size_t pos;
    CycloneLimits limits;
} CycloneReader;

/* Initializes a reader over `[data, data + size)` with the given allocation
 * guards - `cyclone_limits_unlimited()` for none. */
static inline void cyclone_reader_init(CycloneReader *reader, const unsigned char *data,
                                        size_t size, CycloneLimits limits) {
    reader->data = data;
    reader->size = size;
    reader->pos = 0;
    reader->limits = limits;
}

static inline size_t cyclone_reader_remaining(const CycloneReader *reader) {
    return reader->size - reader->pos;
}

/* Whether the field about to be read is **absent** rather than truncated.
 *
 * A generated decoder calls this at every field boundary, and it is the
 * whole of RFC-0002 SS9.1's first rule:
 *
 *   remaining == 0 at a field boundary
 *     -> the writer's model stopped here; this field and every field after
 *        it are absent, and take their zero value. Not an error.
 *
 *   remaining > 0 but fewer bytes than the field needs
 *     -> the field started and the stream ran out inside it. That is a
 *        truncated packet: CYCLONE_DECODE_UNEXPECTED_EOF, never a zero. */
static inline bool cyclone_reader_field_absent(const CycloneReader *reader) {
    return cyclone_reader_remaining(reader) == 0;
}

static inline CycloneDecodeError cyclone_decode_ok(void) {
    CycloneDecodeError error;
    memset(&error, 0, sizeof(error));
    return error;
}

/* Borrows the next `len` bytes and advances the cursor. The single place the
 * remaining-bytes check lives, so no read path can index past the end. */
static inline CycloneDecodeError cyclone_reader_take(CycloneReader *reader, size_t len,
                                                       const unsigned char **out) {
    size_t left = cyclone_reader_remaining(reader);
    if (len > left) {
        CycloneDecodeError error = cyclone_decode_ok();
        error.kind = CYCLONE_DECODE_UNEXPECTED_EOF;
        error.needed = len;
        error.remaining = left;
        return error;
    }
    *out = reader->data + reader->pos;
    reader->pos += len;
    return cyclone_decode_ok();
}

static inline CycloneDecodeError cyclone_reader_read_le(CycloneReader *reader, size_t byte_count,
                                                          uint64_t *out) {
    const unsigned char *bytes = NULL;
    CycloneDecodeError error = cyclone_reader_take(reader, byte_count, &bytes);
    if (!cyclone_decode_error_ok(&error)) {
        return error;
    }
    uint64_t value = 0;
    for (size_t i = 0; i < byte_count; ++i) {
        value |= (uint64_t)bytes[i] << (8 * i);
    }
    *out = value;
    return cyclone_decode_ok();
}

/* Reads a `bool` from 1 byte. Returns `CYCLONE_DECODE_INVALID_BOOL` for any
 * byte but `0x00` and `0x01` - "non-zero means true" is not permitted. */
static inline CycloneDecodeError cyclone_reader_read_bool(CycloneReader *reader, bool *out) {
    size_t start = reader->pos;
    const unsigned char *bytes = NULL;
    CycloneDecodeError error = cyclone_reader_take(reader, 1, &bytes);
    if (!cyclone_decode_error_ok(&error)) {
        return error;
    }
    if (bytes[0] == 0x00) {
        *out = false;
        return cyclone_decode_ok();
    }
    if (bytes[0] == 0x01) {
        *out = true;
        return cyclone_decode_ok();
    }
    reader->pos = start;
    error = cyclone_decode_ok();
    error.kind = CYCLONE_DECODE_INVALID_BOOL;
    error.invalid_byte = bytes[0];
    return error;
}

/* Reads an `i8` from 1 byte. */
static inline CycloneDecodeError cyclone_reader_read_i8(CycloneReader *reader, int8_t *out) {
    const unsigned char *bytes = NULL;
    CycloneDecodeError error = cyclone_reader_take(reader, 1, &bytes);
    if (!cyclone_decode_error_ok(&error)) {
        return error;
    }
    *out = (int8_t)bytes[0];
    return cyclone_decode_ok();
}

/* Reads a `u8` from 1 byte. */
static inline CycloneDecodeError cyclone_reader_read_u8(CycloneReader *reader, uint8_t *out) {
    const unsigned char *bytes = NULL;
    CycloneDecodeError error = cyclone_reader_take(reader, 1, &bytes);
    if (!cyclone_decode_error_ok(&error)) {
        return error;
    }
    *out = bytes[0];
    return cyclone_decode_ok();
}

/* Reads an `i16` from 2 bytes, Little Endian. */
static inline CycloneDecodeError cyclone_reader_read_i16(CycloneReader *reader, int16_t *out) {
    uint64_t value = 0;
    CycloneDecodeError error = cyclone_reader_read_le(reader, 2, &value);
    if (!cyclone_decode_error_ok(&error)) {
        return error;
    }
    *out = (int16_t)value;
    return cyclone_decode_ok();
}

/* Reads a `u16` from 2 bytes, Little Endian. */
static inline CycloneDecodeError cyclone_reader_read_u16(CycloneReader *reader, uint16_t *out) {
    uint64_t value = 0;
    CycloneDecodeError error = cyclone_reader_read_le(reader, 2, &value);
    if (!cyclone_decode_error_ok(&error)) {
        return error;
    }
    *out = (uint16_t)value;
    return cyclone_decode_ok();
}

/* Reads an `i32` from 4 bytes, Little Endian. */
static inline CycloneDecodeError cyclone_reader_read_i32(CycloneReader *reader, int32_t *out) {
    uint64_t value = 0;
    CycloneDecodeError error = cyclone_reader_read_le(reader, 4, &value);
    if (!cyclone_decode_error_ok(&error)) {
        return error;
    }
    *out = (int32_t)value;
    return cyclone_decode_ok();
}

/* Reads a `u32` from 4 bytes, Little Endian. */
static inline CycloneDecodeError cyclone_reader_read_u32(CycloneReader *reader, uint32_t *out) {
    uint64_t value = 0;
    CycloneDecodeError error = cyclone_reader_read_le(reader, 4, &value);
    if (!cyclone_decode_error_ok(&error)) {
        return error;
    }
    *out = (uint32_t)value;
    return cyclone_decode_ok();
}

/* Reads an `i64` from 8 bytes, Little Endian. */
static inline CycloneDecodeError cyclone_reader_read_i64(CycloneReader *reader, int64_t *out) {
    uint64_t value = 0;
    CycloneDecodeError error = cyclone_reader_read_le(reader, 8, &value);
    if (!cyclone_decode_error_ok(&error)) {
        return error;
    }
    *out = (int64_t)value;
    return cyclone_decode_ok();
}

/* Reads a `u64` from 8 bytes, Little Endian. */
static inline CycloneDecodeError cyclone_reader_read_u64(CycloneReader *reader, uint64_t *out) {
    return cyclone_reader_read_le(reader, 8, out);
}

/* Reads an `f32` from its raw 4-byte IEEE 754 bits. The bits are
 * reinterpreted, never normalized. */
static inline CycloneDecodeError cyclone_reader_read_f32(CycloneReader *reader, float *out) {
    uint32_t bits = 0;
    CycloneDecodeError error = cyclone_reader_read_u32(reader, &bits);
    if (!cyclone_decode_error_ok(&error)) {
        return error;
    }
    memcpy(out, &bits, sizeof(*out));
    return cyclone_decode_ok();
}

/* Reads an `f64` from its raw 8-byte IEEE 754 bits. */
static inline CycloneDecodeError cyclone_reader_read_f64(CycloneReader *reader, double *out) {
    uint64_t bits = 0;
    CycloneDecodeError error = cyclone_reader_read_u64(reader, &bits);
    if (!cyclone_decode_error_ok(&error)) {
        return error;
    }
    memcpy(out, &bits, sizeof(*out));
    return cyclone_decode_ok();
}

static inline bool cyclone_is_valid_utf8(const unsigned char *data, size_t len) {
    size_t i = 0;
    while (i < len) {
        unsigned char lead = data[i];
        size_t extra;
        uint32_t minimum;
        uint32_t codepoint;

        if (lead < 0x80) {
            ++i;
            continue;
        }
        if ((lead & 0xE0) == 0xC0) {
            extra = 1;
            minimum = 0x80;
            codepoint = lead & 0x1F;
        } else if ((lead & 0xF0) == 0xE0) {
            extra = 2;
            minimum = 0x800;
            codepoint = lead & 0x0F;
        } else if ((lead & 0xF8) == 0xF0) {
            extra = 3;
            minimum = 0x10000;
            codepoint = lead & 0x07;
        } else {
            return false;
        }

        if (i + extra >= len) {
            return false;
        }
        for (size_t j = 1; j <= extra; ++j) {
            unsigned char continuation = data[i + j];
            if ((continuation & 0xC0) != 0x80) {
                return false;
            }
            codepoint = (codepoint << 6) | (continuation & 0x3F);
        }
        if (codepoint < minimum) {
            return false;
        }
        if (codepoint > 0x10FFFF) {
            return false;
        }
        if (codepoint >= 0xD800 && codepoint <= 0xDFFF) {
            return false;
        }

        i += extra + 1;
    }
    return true;
}

/* Reads a `u32` length prefix and checks it against `limit`. */
static inline CycloneDecodeError cyclone_reader_read_len(CycloneReader *reader, size_t limit,
                                                           size_t *out) {
    size_t start = reader->pos;
    uint32_t len = 0;
    CycloneDecodeError error = cyclone_reader_read_u32(reader, &len);
    if (!cyclone_decode_error_ok(&error)) {
        return error;
    }
    if ((size_t)len > limit) {
        reader->pos = start;
        error = cyclone_decode_ok();
        error.kind = CYCLONE_DECODE_LENGTH_OVERFLOW;
        error.length = len;
        error.limit = limit;
        return error;
    }
    *out = len;
    return cyclone_decode_ok();
}

/* Reads a `string`: a `u32` UTF-8 byte length, then that many bytes, into a
 * freshly `malloc`'d, NUL-terminated buffer. The length is checked against
 * the limit and against the bytes actually remaining **before** anything is
 * allocated (RFC-0002 SS10.1), and the bytes are checked for UTF-8 validity
 * before anything is allocated at all. `*out` is left untouched on any
 * error - including CYCLONE_DECODE_OUT_OF_MEMORY. The caller owns the result
 * and must eventually `free()` it (or, inside a model, let the model's
 * generated `_free` do it).
 *
 * `out` is `const char **`, not `char **`, because a Cyclone `string` field's
 * host type is always `const char *` (see `generator::c`'s module docs) -
 * matching that lets a generated decoder pass `&value->Field` straight
 * through with no cast, which `char **` could not do for a `const char *`
 * field without one. The pointee is still heap-owned and still the caller's
 * to free; `const` here says "do not write through this" (decoded text is
 * logically immutable), not "this is borrowed". */
static inline CycloneDecodeError cyclone_reader_read_string(CycloneReader *reader,
                                                              const char **out) {
    size_t start = reader->pos;
    size_t len = 0;
    CycloneDecodeError error = cyclone_reader_read_len(reader, reader->limits.max_string_len, &len);
    if (!cyclone_decode_error_ok(&error)) {
        return error;
    }

    const unsigned char *bytes = NULL;
    error = cyclone_reader_take(reader, len, &bytes);
    if (!cyclone_decode_error_ok(&error)) {
        reader->pos = start;
        return error;
    }
    if (!cyclone_is_valid_utf8(bytes, len)) {
        reader->pos = start;
        error = cyclone_decode_ok();
        error.kind = CYCLONE_DECODE_INVALID_UTF8;
        return error;
    }

    char *value = (char *)malloc(len + 1);
    if (value == NULL) {
        reader->pos = start;
        error = cyclone_decode_ok();
        error.kind = CYCLONE_DECODE_OUT_OF_MEMORY;
        return error;
    }
    memcpy(value, bytes, len);
    value[len] = '\0';
    *out = value;
    return cyclone_decode_ok();
}

/* Reads a `bytes` blob: a `u32` length, then that many raw bytes, into a
 * freshly `malloc`'d [`CycloneBytes`]. `*out` is left untouched on error. */
static inline CycloneDecodeError cyclone_reader_read_bytes(CycloneReader *reader,
                                                             CycloneBytes *out) {
    size_t start = reader->pos;
    size_t len = 0;
    CycloneDecodeError error = cyclone_reader_read_len(reader, reader->limits.max_bytes_len, &len);
    if (!cyclone_decode_error_ok(&error)) {
        return error;
    }

    const unsigned char *bytes = NULL;
    error = cyclone_reader_take(reader, len, &bytes);
    if (!cyclone_decode_error_ok(&error)) {
        reader->pos = start;
        return error;
    }

    if (len == 0) {
        out->data = NULL;
        out->len = 0;
        return cyclone_decode_ok();
    }
    unsigned char *value = (unsigned char *)malloc(len);
    if (value == NULL) {
        reader->pos = start;
        error = cyclone_decode_ok();
        error.kind = CYCLONE_DECODE_OUT_OF_MEMORY;
        return error;
    }
    memcpy(value, bytes, len);
    out->data = value;
    out->len = len;
    return cyclone_decode_ok();
}

/* Reads an `Array<T>`'s element count (RFC-0002 SS6), checked against
 * `limits.max_array_count` before the caller reads a single element. */
static inline CycloneDecodeError cyclone_reader_read_array_count(CycloneReader *reader,
                                                                   size_t *out) {
    return cyclone_reader_read_len(reader, reader->limits.max_array_count, out);
}
"####;
