//! The Cyclone runtime, carried verbatim into `runtime.js`.
//!
//! The JavaScript counterpart of [`super::typescript_runtime`] - the same
//! block, with every type annotation erased and TypeScript-only syntax
//! (`interface`, visibility modifiers) replaced by its plain-JavaScript
//! equivalent (`#private` class fields, JSDoc `@param`/`@returns` comments
//! for a reader that wants the types without a compiler). See
//! [`super::typescript_runtime`]'s module docs for why `DataView` and
//! `bigint` are used at all - every word of that reasoning applies here
//! unchanged, because this **is** that reasoning, generating plain
//! JavaScript instead of TypeScript.
//!
//! Kept a byte-for-byte identical wire format on purpose: the brief requires
//! it, and it is checked by generating both from the same schema and
//! comparing what they encode - see `tests/cli.rs`'s
//! `typescript_and_javascript_backends_agree_on_the_wire_format`.

/// The runtime block, emitted once, into its own file.
pub const RUNTIME: &str = r####"
// ==========================================================================
// Cyclone runtime - RFC-0002, carried verbatim.
//
// Not generated from your models: this block is identical in every project
// cyclonec generates for. It is here so the generated tree is self-contained -
// nothing to add to package.json, nothing to import from elsewhere.
// ==========================================================================

const CYCLONE_TEXT_ENCODER = new TextEncoder();
const CYCLONE_TEXT_DECODER = new TextDecoder("utf-8", { fatal: true });

/** A byte stream that does not satisfy the Cyclone Specification. */
export class DecodeError extends Error {
    constructor(message) {
        super(message);
        this.name = "DecodeError";
    }

    /**
     * Fewer bytes remain than the value being read requires, **after the
     * read had already begun**.
     *
     * Bytes running out exactly on a field boundary is not this error - it
     * is version skew (RFC-0002 §9.1), and the generated decoder handles it
     * without asking the runtime.
     *
     * @param {number} needed
     * @param {number} remaining
     * @returns {DecodeError}
     */
    static unexpectedEof(needed, remaining) {
        return new DecodeError(`unexpected eof: needed ${needed} bytes, ${remaining} remaining`);
    }

    /**
     * A `bool` byte that is neither `0x00` nor `0x01` (RFC-0002 §2.4).
     *
     * @param {number} value
     * @returns {DecodeError}
     */
    static invalidBool(value) {
        return new DecodeError(
            `invalid bool: 0x${value.toString(16).padStart(2, "0")} is neither 0x00 nor 0x01`,
        );
    }

    /** A `string` region that is not valid UTF-8.
     * @returns {DecodeError}
     */
    static invalidUtf8() {
        return new DecodeError("invalid utf-8 in string");
    }

    /**
     * A length field beyond the configured limit.
     *
     * @param {number} length
     * @param {number} limit
     * @returns {DecodeError}
     */
    static lengthOverflow(length, limit) {
        return new DecodeError(`length overflow: length ${length} exceeds limit ${limit}`);
    }
}

/**
 * Allocation guards applied while decoding (RFC-0002 §12).
 *
 * A `u32` length can claim up to 4 GiB, so a decoder that allocates straight
 * from an untrusted one is a denial-of-service target. These are **not
 * part of the wire format**: two peers with different limits may disagree
 * about a byte stream, and neither is wrong.
 */
export class Limits {
    /**
     * @param {number} maxStringLen Largest accepted UTF-8 byte length of a `string`.
     * @param {number} maxBytesLen Largest accepted byte length of a `bytes` blob.
     * @param {number} maxArrayCount Largest accepted element count of an `Array<T>` (RFC-0002 §6).
     */
    constructor(maxStringLen, maxBytesLen, maxArrayCount) {
        this.maxStringLen = maxStringLen;
        this.maxBytesLen = maxBytesLen;
        this.maxArrayCount = maxArrayCount;
    }
}

/** The permissive default: `0xFFFFFFFF` for every field. */
Limits.UNLIMITED = new Limits(0xffffffff, 0xffffffff, 0xffffffff);

/**
 * Appends Cyclone-encoded values to a growable buffer.
 *
 * Every multi-byte value is Little Endian, with no padding, no alignment and
 * no metadata between values.
 */
export class Writer {
    #bytes;
    #view;
    #len;

    constructor(capacity = 64) {
        this.#bytes = new Uint8Array(Math.max(capacity, 1));
        this.#view = new DataView(this.#bytes.buffer);
        this.#len = 0;
    }

    /** The number of bytes written so far. */
    get length() {
        return this.#len;
    }

    /**
     * The bytes written so far, copied out.
     * @returns {Uint8Array}
     */
    toUint8Array() {
        return this.#bytes.slice(0, this.#len);
    }

    #ensure(extra) {
        if (this.#len + extra <= this.#bytes.length) {
            return;
        }
        let capacity = this.#bytes.length * 2;
        while (capacity < this.#len + extra) {
            capacity *= 2;
        }
        const grown = new Uint8Array(capacity);
        grown.set(this.#bytes.subarray(0, this.#len));
        this.#bytes = grown;
        this.#view = new DataView(this.#bytes.buffer);
    }

    /** Writes a `bool` as one byte: `0x00` or `0x01`, never anything else.
     * @param {boolean} value */
    writeBool(value) {
        this.#ensure(1);
        this.#view.setUint8(this.#len, value ? 1 : 0);
        this.#len += 1;
    }

    /** Writes an `i8` as 1 byte.
     * @param {number} value */
    writeI8(value) {
        this.#ensure(1);
        this.#view.setInt8(this.#len, value);
        this.#len += 1;
    }

    /** Writes a `u8` as 1 byte.
     * @param {number} value */
    writeU8(value) {
        this.#ensure(1);
        this.#view.setUint8(this.#len, value);
        this.#len += 1;
    }

    /** Writes an `i16` as 2 bytes, Little Endian.
     * @param {number} value */
    writeI16(value) {
        this.#ensure(2);
        this.#view.setInt16(this.#len, value, true);
        this.#len += 2;
    }

    /** Writes a `u16` as 2 bytes, Little Endian.
     * @param {number} value */
    writeU16(value) {
        this.#ensure(2);
        this.#view.setUint16(this.#len, value, true);
        this.#len += 2;
    }

    /** Writes an `i32` as 4 bytes, Little Endian.
     * @param {number} value */
    writeI32(value) {
        this.#ensure(4);
        this.#view.setInt32(this.#len, value, true);
        this.#len += 4;
    }

    /** Writes a `u32` as 4 bytes, Little Endian.
     * @param {number} value */
    writeU32(value) {
        this.#ensure(4);
        this.#view.setUint32(this.#len, value, true);
        this.#len += 4;
    }

    /** Writes an `i64` as 8 bytes, Little Endian, from a `bigint`.
     * @param {bigint} value */
    writeI64(value) {
        this.#ensure(8);
        this.#view.setBigInt64(this.#len, value, true);
        this.#len += 8;
    }

    /** Writes a `u64` as 8 bytes, Little Endian, from a `bigint`.
     * @param {bigint} value */
    writeU64(value) {
        this.#ensure(8);
        this.#view.setBigUint64(this.#len, value, true);
        this.#len += 8;
    }

    /**
     * Writes an `f32` as its raw IEEE 754 bits, 4 bytes Little Endian.
     *
     * The bit pattern is written unmodified: `NaN` payloads survive and
     * `-0.0` stays distinct from `0.0`.
     * @param {number} value
     */
    writeF32(value) {
        this.#ensure(4);
        this.#view.setFloat32(this.#len, value, true);
        this.#len += 4;
    }

    /** Writes an `f64` as its raw IEEE 754 bits, 8 bytes Little Endian.
     * @param {number} value */
    writeF64(value) {
        this.#ensure(8);
        this.#view.setFloat64(this.#len, value, true);
        this.#len += 8;
    }

    /**
     * Writes a `string` as a `u32` UTF-8 **byte** length, then those bytes.
     *
     * The length counts bytes, not characters.
     * @param {string} value
     */
    writeString(value) {
        const encoded = CYCLONE_TEXT_ENCODER.encode(value);
        this.#writeLength(encoded.length);
        this.#ensure(encoded.length);
        this.#bytes.set(encoded, this.#len);
        this.#len += encoded.length;
    }

    /** Writes a `bytes` blob as a `u32` length, then the raw bytes.
     * @param {Uint8Array} value */
    writeBytes(value) {
        this.#writeLength(value.length);
        this.#ensure(value.length);
        this.#bytes.set(value, this.#len);
        this.#len += value.length;
    }

    /**
     * Writes an `Array<T>`'s element count (RFC-0002 §6) - the caller writes
     * each element itself, in order, right after.
     * @param {number} count
     */
    writeArrayCount(count) {
        this.#writeLength(count);
    }

    #writeLength(len) {
        if (len > 0xffffffff) {
            throw new RangeError(
                "cyclone: length exceeds 0xFFFFFFFF and cannot be represented on the wire",
            );
        }
        this.writeU32(len);
    }
}

/**
 * Reads Cyclone-encoded values from a borrowed buffer.
 *
 * Malformed input is always a {@link DecodeError}, never a silent wrong
 * answer, and a failed read leaves the cursor where it was.
 */
export class Reader {
    #bytes;
    #view;
    #pos;
    #limits;

    /**
     * Creates a reader over `bytes` with {@link Limits.UNLIMITED}.
     * @param {Uint8Array} bytes
     * @param {Limits} [limits]
     */
    constructor(bytes, limits = Limits.UNLIMITED) {
        this.#bytes = bytes;
        this.#view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
        this.#pos = 0;
        this.#limits = limits;
    }

    /** The cursor position, in bytes from the start. */
    get position() {
        return this.#pos;
    }

    /** The number of bytes left to read. */
    get remaining() {
        return this.#bytes.length - this.#pos;
    }

    /** Whether the cursor has reached the end. */
    get isEmpty() {
        return this.remaining === 0;
    }

    /**
     * Whether the field about to be read is **absent** rather than
     * truncated.
     *
     * A generated decoder calls this at every field boundary, and it is the
     * whole of RFC-0002 §9.1's first rule:
     *
     * ```text
     * remaining === 0 at a field boundary
     *   -> the writer's model stopped here; this field and every field after
     *      it are absent, and take their zero value. Not an error.
     *
     * remaining > 0 but fewer bytes than the field needs
     *   -> the field started and the stream ran out inside it. That is a
     *      truncated packet: DecodeError, never a zero.
     * ```
     *
     * The distinction is the reason this method exists. Treating a partial
     * field as a zero would hide packet corruption behind a plausible value.
     * @returns {boolean}
     */
    fieldAbsent() {
        return this.remaining === 0;
    }

    /** The limits this reader enforces.
     * @returns {Limits} */
    getLimits() {
        return this.#limits;
    }

    /**
     * Reads a `bool` from 1 byte.
     *
     * Throws {@link DecodeError} for any byte but `0x00` and `0x01` -
     * "non-zero means true" is not permitted.
     * @returns {boolean}
     */
    readBool() {
        const value = this.readU8();
        if (value === 0x00) {
            return false;
        }
        if (value === 0x01) {
            return true;
        }
        this.#pos -= 1;
        throw DecodeError.invalidBool(value);
    }

    /** Reads an `i8` from 1 byte.
     * @returns {number} */
    readI8() {
        return this.#view.getInt8(this.#take(1));
    }

    /** Reads a `u8` from 1 byte.
     * @returns {number} */
    readU8() {
        return this.#view.getUint8(this.#take(1));
    }

    /** Reads an `i16` from 2 bytes, Little Endian.
     * @returns {number} */
    readI16() {
        return this.#view.getInt16(this.#take(2), true);
    }

    /** Reads a `u16` from 2 bytes, Little Endian.
     * @returns {number} */
    readU16() {
        return this.#view.getUint16(this.#take(2), true);
    }

    /** Reads an `i32` from 4 bytes, Little Endian.
     * @returns {number} */
    readI32() {
        return this.#view.getInt32(this.#take(4), true);
    }

    /** Reads a `u32` from 4 bytes, Little Endian.
     * @returns {number} */
    readU32() {
        return this.#view.getUint32(this.#take(4), true);
    }

    /** Reads an `i64` from 8 bytes, Little Endian, as a `bigint`.
     * @returns {bigint} */
    readI64() {
        return this.#view.getBigInt64(this.#take(8), true);
    }

    /** Reads a `u64` from 8 bytes, Little Endian, as a `bigint`.
     * @returns {bigint} */
    readU64() {
        return this.#view.getBigUint64(this.#take(8), true);
    }

    /**
     * Reads an `f32` from its raw 4-byte IEEE 754 bits.
     *
     * The bits are reinterpreted, never normalized.
     * @returns {number}
     */
    readF32() {
        return this.#view.getFloat32(this.#take(4), true);
    }

    /** Reads an `f64` from its raw 8-byte IEEE 754 bits.
     * @returns {number} */
    readF64() {
        return this.#view.getFloat64(this.#take(8), true);
    }

    /**
     * Reads a `string`: a `u32` UTF-8 byte length, then that many bytes.
     *
     * The length is checked against the limit and against the bytes actually
     * remaining **before** anything is decoded (RFC-0002 §10.1).
     * @returns {string}
     */
    readString() {
        const start = this.#pos;
        const len = this.#readLength(this.#limits.maxStringLen);

        let offset;
        try {
            offset = this.#take(len);
        } catch (error) {
            this.#pos = start;
            throw error;
        }

        try {
            return CYCLONE_TEXT_DECODER.decode(this.#bytes.subarray(offset, offset + len));
        } catch {
            this.#pos = start;
            throw DecodeError.invalidUtf8();
        }
    }

    /** Reads a `bytes` blob: a `u32` length, then that many raw bytes.
     * @returns {Uint8Array} */
    readBytes() {
        const start = this.#pos;
        const len = this.#readLength(this.#limits.maxBytesLen);

        let offset;
        try {
            offset = this.#take(len);
        } catch (error) {
            this.#pos = start;
            throw error;
        }

        return this.#bytes.slice(offset, offset + len);
    }

    /**
     * Reads an `Array<T>`'s element count (RFC-0002 §6), checked against
     * {@link Limits#maxArrayCount} before the caller reads a single element.
     * @returns {number}
     */
    readArrayCount() {
        return this.#readLength(this.#limits.maxArrayCount);
    }

    #readLength(limit) {
        const start = this.#pos;
        const len = this.readU32();
        if (len > limit) {
            this.#pos = start;
            throw DecodeError.lengthOverflow(len, limit);
        }
        return len;
    }

    /**
     * Borrows the next `len` bytes and advances the cursor, returning the
     * offset they start at.
     *
     * The single place the remaining-bytes check lives, so no read path can
     * read past the end - and the only one that ever moves `pos`, so a
     * failed read always leaves it exactly where it was.
     */
    #take(len) {
        const remaining = this.remaining;
        if (len > remaining) {
            throw DecodeError.unexpectedEof(len, remaining);
        }
        const offset = this.#pos;
        this.#pos += len;
        return offset;
    }
}
"####;
