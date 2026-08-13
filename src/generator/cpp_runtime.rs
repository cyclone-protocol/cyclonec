//! The Cyclone runtime, carried verbatim into `runtime.hpp`.
//!
//! The C++ counterpart of [`super::rust_runtime`], [`super::go_runtime`] and
//! [`super::csharp_runtime`] - same reasoning, same guarantee: the block below
//! is fixed, written once against RFC-0002, and copied out unchanged. Nothing
//! about byte layout is computed per model, per field, or per run.
//!
//! # Why an error is returned, not thrown
//!
//! Unlike [`super::csharp_runtime`], this runtime never throws. A generated
//! model's fields are plain public members - not properties - so nothing here
//! needed C#'s workaround either; what it *does* need is a shape a C++
//! project that has turned exceptions off (common in game and embedded code,
//! and the whole reason `-fno-exceptions` exists) can still call. Every
//! `Reader` read method therefore takes its result by output reference and
//! returns a [`DecodeError`], the C++ counterpart of Go's `(T, error)` and
//! Rust's `Result<T, DecodeError>`: a default-constructed `DecodeError` *is*
//! "no error" (`kind == DecodeError::Kind::None`), so a caller checks
//! `error.ok()` exactly the way a generated Go decoder checks `err != nil`.
//!
//! # Why a length prefix needs no 64-bit workaround
//!
//! [`super::gdscript_runtime`] had to split a 64-bit fingerprint constant in
//! two because GDScript's `int` has no unsigned counterpart. C++'s
//! `std::uint64_t` is exact and unsigned, and an unsuffixed hex literal too
//! wide for `int`/`long` is promoted to `unsigned long long` by the language
//! itself (an ordinary rule of `[lex.icon]`, not something this generator has
//! to work around) - so a fingerprint is written as a plain
//! `0x...ULL` literal, the `ULL` suffix only ever there for clarity.
//!
//! # Endianness is never assumed
//!
//! Every multi-byte value is assembled and disassembled byte by byte with
//! shifts and masks, never `memcpy`d as a whole and never read through a
//! reinterpreted pointer - so the wire format is Little Endian on a
//! Little-Endian host and a Big-Endian one alike. `memcpy` is used in exactly
//! one place, [`Writer::write_f32`]/[`Writer::write_f64`] and their `Reader`
//! counterparts, and only to reinterpret an IEEE-754 value's bits as an
//! integer of the same width - never to move multi-byte integers to or from
//! the wire, which is what would make the result host-endianness-dependent.

/// The runtime block, emitted once, into its own file, inside the generated
/// namespace.
pub const RUNTIME: &str = r####"
// ==========================================================================
// Cyclone runtime - RFC-0002, carried verbatim.
//
// Not generated from your models: this block is identical in every project
// cyclonec generates for. It is here so the generated tree is self-contained
// - nothing to add to your build beyond this directory, nothing to link.
// ==========================================================================

/// A byte stream that does not satisfy the Cyclone Specification.
///
/// A default-constructed `DecodeError` means "no error" - `kind` is
/// `Kind::None` and `ok()` is true - which is what every generated `decode`
/// returns on success, the same value a fresh local starts as.
struct DecodeError {
    enum class Kind {
        None,
        /// Fewer bytes remain than the value being read requires, **after
        /// the read had already begun**.
        ///
        /// Bytes running out exactly on a field boundary is not this error -
        /// it is version skew (RFC-0002 §9.1), and the generated decoder
        /// handles it without asking the runtime.
        UnexpectedEof,
        /// A `bool` byte that is neither `0x00` nor `0x01` (RFC-0002 §2.4).
        InvalidBool,
        /// A `string` region that is not valid UTF-8.
        InvalidUtf8,
        /// A length field beyond the configured limit.
        LengthOverflow,
    };

    Kind kind = Kind::None;
    /// `UnexpectedEof`: bytes the read needed.
    std::size_t needed = 0;
    /// `UnexpectedEof`: bytes actually left.
    std::size_t remaining = 0;
    /// `InvalidBool`: the byte that was neither `0x00` nor `0x01`.
    std::uint8_t invalid_byte = 0;
    /// `LengthOverflow`: the length read from the stream.
    std::size_t length = 0;
    /// `LengthOverflow`: the limit it exceeded.
    std::size_t limit = 0;

    /// Whether this is "no error" - the value a fresh `DecodeError` starts
    /// as, and the value every generated `decode` returns on success.
    bool ok() const { return kind == Kind::None; }

    /// A human-readable description, for logging - never parsed back into a
    /// `DecodeError`, so its wording is not part of any contract.
    std::string message() const {
        char buffer[160];
        switch (kind) {
            case Kind::None:
                return "no error";
            case Kind::UnexpectedEof:
                std::snprintf(
                    buffer, sizeof(buffer),
                    "unexpected eof: needed %zu bytes, %zu remaining",
                    needed, remaining);
                return buffer;
            case Kind::InvalidBool:
                std::snprintf(
                    buffer, sizeof(buffer),
                    "invalid bool: 0x%02X is neither 0x00 nor 0x01",
                    static_cast<unsigned>(invalid_byte));
                return buffer;
            case Kind::InvalidUtf8:
                return "invalid utf-8 in string";
            case Kind::LengthOverflow:
                std::snprintf(
                    buffer, sizeof(buffer),
                    "length overflow: length %zu exceeds limit %zu",
                    length, limit);
                return buffer;
        }
        return "unknown decode error";
    }
};

/// Allocation guards applied while decoding (RFC-0002 §12).
///
/// A `u32` length can claim up to 4 GiB, so a decoder that allocates straight
/// from an untrusted one is a denial-of-service target. These are **not part
/// of the wire format**: two peers with different limits may disagree about a
/// byte stream, and neither is wrong.
struct Limits {
    /// Largest accepted UTF-8 byte length of a `string`.
    std::size_t max_string_len = 0xFFFFFFFFu;
    /// Largest accepted byte length of a `bytes` blob.
    std::size_t max_bytes_len = 0xFFFFFFFFu;
    /// Largest accepted element count of an `Array<T>` (RFC-0002 §6).
    std::size_t max_array_count = 0xFFFFFFFFu;

    /// The permissive default: every field's own default is already
    /// `0xFFFFFFFF` - `u32::MAX`, the widest length RFC-0002 can even
    /// express - so a default-constructed `Limits` already means
    /// "unlimited". This spells that out for a reader who does not want to
    /// have to know that.
    static Limits unlimited() { return Limits(); }
};

/// Appends Cyclone-encoded values to a growable buffer.
///
/// Every multi-byte value is Little Endian, with no padding, no alignment and
/// no metadata between values.
class Writer {
public:
    Writer() = default;

    /// Creates an empty writer with room for `capacity` bytes.
    explicit Writer(std::size_t capacity) { buf_.reserve(capacity); }

    /// The bytes written so far.
    const std::vector<std::uint8_t>& bytes() const { return buf_; }

    /// Consumes the writer and returns the bytes.
    std::vector<std::uint8_t> into_bytes() { return std::move(buf_); }

    /// The number of bytes written so far.
    std::size_t size() const { return buf_.size(); }

    /// Whether nothing has been written yet.
    bool empty() const { return buf_.empty(); }

    /// Discards everything written, keeping the capacity.
    void clear() { buf_.clear(); }

    /// Writes a `bool` as one byte: `0x00` or `0x01`, never anything else.
    void write_bool(bool value) { buf_.push_back(value ? std::uint8_t{1} : std::uint8_t{0}); }

    /// Writes an `i8` as 1 byte.
    void write_i8(std::int8_t value) { buf_.push_back(static_cast<std::uint8_t>(value)); }

    /// Writes a `u8` as 1 byte.
    void write_u8(std::uint8_t value) { buf_.push_back(value); }

    /// Writes an `i16` as 2 bytes, Little Endian.
    void write_i16(std::int16_t value) { write_u16(static_cast<std::uint16_t>(value)); }

    /// Writes a `u16` as 2 bytes, Little Endian.
    void write_u16(std::uint16_t value) { write_little_endian(value, 2); }

    /// Writes an `i32` as 4 bytes, Little Endian.
    void write_i32(std::int32_t value) { write_u32(static_cast<std::uint32_t>(value)); }

    /// Writes a `u32` as 4 bytes, Little Endian.
    void write_u32(std::uint32_t value) { write_little_endian(value, 4); }

    /// Writes an `i64` as 8 bytes, Little Endian.
    void write_i64(std::int64_t value) { write_u64(static_cast<std::uint64_t>(value)); }

    /// Writes a `u64` as 8 bytes, Little Endian.
    void write_u64(std::uint64_t value) { write_little_endian(value, 8); }

    /// Writes an `f32` as its raw IEEE 754 bits, 4 bytes Little Endian.
    ///
    /// The bit pattern is written unmodified: `NaN` payloads survive and
    /// `-0.0` stays distinct from `0.0`.
    void write_f32(float value) {
        std::uint32_t bits;
        std::memcpy(&bits, &value, sizeof(bits));
        write_u32(bits);
    }

    /// Writes an `f64` as its raw IEEE 754 bits, 8 bytes Little Endian.
    void write_f64(double value) {
        std::uint64_t bits;
        std::memcpy(&bits, &value, sizeof(bits));
        write_u64(bits);
    }

    /// Writes a `string` as a `u32` UTF-8 **byte** length, then those bytes.
    ///
    /// The length counts bytes, not characters.
    void write_string(const std::string& value) {
        write_len(value.size());
        buf_.insert(buf_.end(), value.begin(), value.end());
    }

    /// Writes a `bytes` blob as a `u32` length, then the raw bytes.
    void write_bytes(const std::vector<std::uint8_t>& value) {
        write_len(value.size());
        buf_.insert(buf_.end(), value.begin(), value.end());
    }

    /// Writes an `Array<T>`'s element count (RFC-0002 §6) - the caller writes
    /// each element itself, in order, right after.
    void write_array_count(std::size_t count) { write_len(count); }

private:
    void write_len(std::size_t len) { write_u32(static_cast<std::uint32_t>(len)); }

    void write_little_endian(std::uint64_t value, int byte_count) {
        for (int i = 0; i < byte_count; ++i) {
            buf_.push_back(static_cast<std::uint8_t>(value >> (8 * i)));
        }
    }

    std::vector<std::uint8_t> buf_;
};

/// Reads Cyclone-encoded values from a borrowed buffer.
///
/// Malformed input is always a [`DecodeError`], never a thrown exception and
/// never a silent wrong answer, and a failed read leaves the cursor where it
/// was. The buffer is borrowed - a `Reader` never outlives the bytes it was
/// constructed over, the same discipline `std::string_view` asks of its
/// caller.
class Reader {
public:
    /// Creates a reader over `[data, data + size)` with the unlimited
    /// default limits.
    Reader(const std::uint8_t* data, std::size_t size) : data_(data), size_(size) {}

    /// Creates a reader over `[data, data + size)` with explicit allocation
    /// guards.
    Reader(const std::uint8_t* data, std::size_t size, Limits limits)
        : data_(data), size_(size), limits_(limits) {}

    /// The cursor position, in bytes from the start.
    std::size_t position() const { return pos_; }

    /// The number of bytes left to read.
    std::size_t remaining() const { return size_ - pos_; }

    /// Whether the cursor has reached the end.
    bool is_empty() const { return remaining() == 0; }

    /// Whether the field about to be read is **absent** rather than
    /// truncated.
    ///
    /// A generated decoder calls this at every field boundary, and it is the
    /// whole of RFC-0002 §9.1's first rule:
    ///
    ///   remaining() == 0 at a field boundary
    ///     -> the writer's model stopped here; this field and every field
    ///        after it are absent, and take their zero value. Not an error.
    ///
    ///   remaining() > 0 but fewer bytes than the field needs
    ///     -> the field started and the stream ran out inside it. That is a
    ///        truncated packet: DecodeError::Kind::UnexpectedEof, never a
    ///        zero.
    ///
    /// The distinction is the reason this method exists. Treating a partial
    /// field as a zero would hide packet corruption behind a plausible
    /// value.
    bool field_absent() const { return remaining() == 0; }

    /// The limits this reader enforces.
    Limits limits() const { return limits_; }

    /// Reads a `bool` from 1 byte.
    ///
    /// Returns `DecodeError::Kind::InvalidBool` for any byte but `0x00` and
    /// `0x01` - "non-zero means true" is not permitted.
    DecodeError read_bool(bool& out) {
        std::uint8_t byte = 0;
        if (DecodeError error = read_u8(byte); !error.ok()) return error;
        if (byte == 0x00) { out = false; return DecodeError{}; }
        if (byte == 0x01) { out = true; return DecodeError{}; }
        pos_ -= 1;
        DecodeError error;
        error.kind = DecodeError::Kind::InvalidBool;
        error.invalid_byte = byte;
        return error;
    }

    /// Reads an `i8` from 1 byte.
    DecodeError read_i8(std::int8_t& out) {
        const std::uint8_t* bytes = nullptr;
        if (DecodeError error = take(1, bytes); !error.ok()) return error;
        out = static_cast<std::int8_t>(bytes[0]);
        return DecodeError{};
    }

    /// Reads a `u8` from 1 byte.
    DecodeError read_u8(std::uint8_t& out) {
        const std::uint8_t* bytes = nullptr;
        if (DecodeError error = take(1, bytes); !error.ok()) return error;
        out = bytes[0];
        return DecodeError{};
    }

    /// Reads an `i16` from 2 bytes, Little Endian.
    DecodeError read_i16(std::int16_t& out) {
        std::uint16_t value = 0;
        if (DecodeError error = read_u16(value); !error.ok()) return error;
        out = static_cast<std::int16_t>(value);
        return DecodeError{};
    }

    /// Reads a `u16` from 2 bytes, Little Endian.
    DecodeError read_u16(std::uint16_t& out) {
        std::uint64_t value = 0;
        if (DecodeError error = read_little_endian(2, value); !error.ok()) return error;
        out = static_cast<std::uint16_t>(value);
        return DecodeError{};
    }

    /// Reads an `i32` from 4 bytes, Little Endian.
    DecodeError read_i32(std::int32_t& out) {
        std::uint32_t value = 0;
        if (DecodeError error = read_u32(value); !error.ok()) return error;
        out = static_cast<std::int32_t>(value);
        return DecodeError{};
    }

    /// Reads a `u32` from 4 bytes, Little Endian.
    DecodeError read_u32(std::uint32_t& out) {
        std::uint64_t value = 0;
        if (DecodeError error = read_little_endian(4, value); !error.ok()) return error;
        out = static_cast<std::uint32_t>(value);
        return DecodeError{};
    }

    /// Reads an `i64` from 8 bytes, Little Endian.
    DecodeError read_i64(std::int64_t& out) {
        std::uint64_t value = 0;
        if (DecodeError error = read_u64(value); !error.ok()) return error;
        out = static_cast<std::int64_t>(value);
        return DecodeError{};
    }

    /// Reads a `u64` from 8 bytes, Little Endian.
    DecodeError read_u64(std::uint64_t& out) {
        return read_little_endian(8, out);
    }

    /// Reads an `f32` from its raw 4-byte IEEE 754 bits.
    ///
    /// The bits are reinterpreted, never normalized.
    DecodeError read_f32(float& out) {
        std::uint32_t bits = 0;
        if (DecodeError error = read_u32(bits); !error.ok()) return error;
        std::memcpy(&out, &bits, sizeof(out));
        return DecodeError{};
    }

    /// Reads an `f64` from its raw 8-byte IEEE 754 bits.
    DecodeError read_f64(double& out) {
        std::uint64_t bits = 0;
        if (DecodeError error = read_u64(bits); !error.ok()) return error;
        std::memcpy(&out, &bits, sizeof(out));
        return DecodeError{};
    }

    /// Reads a `string`: a `u32` UTF-8 byte length, then that many bytes.
    ///
    /// The length is checked against the limit and against the bytes
    /// actually remaining **before** anything is allocated (RFC-0002 §10.1),
    /// and the bytes are checked for UTF-8 validity before `out` is touched.
    DecodeError read_string(std::string& out) {
        std::size_t start = pos_;
        std::size_t len = 0;
        if (DecodeError error = read_len(limits_.max_string_len, len); !error.ok()) return error;

        const std::uint8_t* bytes = nullptr;
        if (DecodeError error = take(len, bytes); !error.ok()) {
            pos_ = start;
            return error;
        }
        if (!is_valid_utf8(bytes, len)) {
            pos_ = start;
            DecodeError error;
            error.kind = DecodeError::Kind::InvalidUtf8;
            return error;
        }
        out.assign(reinterpret_cast<const char*>(bytes), len);
        return DecodeError{};
    }

    /// Reads a `bytes` blob: a `u32` length, then that many raw bytes.
    DecodeError read_bytes(std::vector<std::uint8_t>& out) {
        std::size_t start = pos_;
        std::size_t len = 0;
        if (DecodeError error = read_len(limits_.max_bytes_len, len); !error.ok()) return error;

        const std::uint8_t* bytes = nullptr;
        if (DecodeError error = take(len, bytes); !error.ok()) {
            pos_ = start;
            return error;
        }
        out.assign(bytes, bytes + len);
        return DecodeError{};
    }

    /// Reads an `Array<T>`'s element count (RFC-0002 §6), checked against
    /// `Limits::max_array_count` before the caller reads a single element.
    DecodeError read_array_count(std::size_t& out) {
        return read_len(limits_.max_array_count, out);
    }

private:
    /// Reads a `u32` length prefix and checks it against `limit`.
    DecodeError read_len(std::size_t limit, std::size_t& out) {
        std::size_t start = pos_;
        std::uint32_t len = 0;
        if (DecodeError error = read_u32(len); !error.ok()) return error;
        if (static_cast<std::size_t>(len) > limit) {
            pos_ = start;
            DecodeError error;
            error.kind = DecodeError::Kind::LengthOverflow;
            error.length = len;
            error.limit = limit;
            return error;
        }
        out = len;
        return DecodeError{};
    }

    DecodeError read_little_endian(int byte_count, std::uint64_t& out) {
        const std::uint8_t* bytes = nullptr;
        if (DecodeError error = take(static_cast<std::size_t>(byte_count), bytes); !error.ok()) {
            return error;
        }
        std::uint64_t value = 0;
        for (int i = 0; i < byte_count; ++i) {
            value |= static_cast<std::uint64_t>(bytes[i]) << (8 * i);
        }
        out = value;
        return DecodeError{};
    }

    /// Borrows the next `len` bytes and advances the cursor.
    ///
    /// The single place the remaining-bytes check lives, so no read path can
    /// index past the end.
    DecodeError take(std::size_t len, const std::uint8_t*& out) {
        std::size_t left = remaining();
        if (len > left) {
            DecodeError error;
            error.kind = DecodeError::Kind::UnexpectedEof;
            error.needed = len;
            error.remaining = left;
            return error;
        }
        out = data_ + pos_;
        pos_ += len;
        return DecodeError{};
    }

    static bool is_valid_utf8(const std::uint8_t* data, std::size_t len) {
        std::size_t i = 0;
        while (i < len) {
            std::uint8_t lead = data[i];
            std::size_t extra;
            std::uint32_t minimum;
            std::uint32_t codepoint;

            if (lead < 0x80) { ++i; continue; }
            if ((lead & 0xE0) == 0xC0) { extra = 1; minimum = 0x80; codepoint = lead & 0x1F; }
            else if ((lead & 0xF0) == 0xE0) { extra = 2; minimum = 0x800; codepoint = lead & 0x0F; }
            else if ((lead & 0xF8) == 0xF0) { extra = 3; minimum = 0x10000; codepoint = lead & 0x07; }
            else { return false; }

            if (i + extra >= len) return false;
            for (std::size_t j = 1; j <= extra; ++j) {
                std::uint8_t continuation = data[i + j];
                if ((continuation & 0xC0) != 0x80) return false;
                codepoint = (codepoint << 6) | (continuation & 0x3F);
            }
            if (codepoint < minimum) return false;
            if (codepoint > 0x10FFFF) return false;
            if (codepoint >= 0xD800 && codepoint <= 0xDFFF) return false;

            i += extra + 1;
        }
        return true;
    }

    const std::uint8_t* data_ = nullptr;
    std::size_t size_ = 0;
    std::size_t pos_ = 0;
    Limits limits_{};
};
"####;
