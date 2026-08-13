//! The Cyclone runtime, carried verbatim into `runtime.rs`.
//!
//! # Why this is a constant
//!
//! The generator is forbidden from working out byte layout, endianness or
//! string encoding, and it does not: the text below is fixed, written once
//! against RFC-0002, and copied out unchanged. Nothing here is computed per
//! model, per field, or per run - the generator cannot derive a wire format
//! even in principle, because it only knows how to `push_str` this.
//!
//! Everything is spelled with fully-qualified paths (`::core::…`, `::std::…`)
//! so the block introduces no `use` of its own and cannot collide with the
//! imports at the site it is included into.
//!
//! # What changed from `cyclonec_old`
//!
//! One method: [`Reader::field_absent`]. The old runtime gave a generated
//! decoder no way to tell *this field never arrived* from *this field arrived
//! truncated* - every read simply returned `UnexpectedEof` - so the decoder
//! could not implement RFC-0002 §9.1 at all. See `generator::rust` for what the
//! generated decoder does with it.

/// The runtime block, emitted once, into its own file.
pub const RUNTIME: &str = r####"
// ==========================================================================
// Cyclone runtime - RFC-0002, carried verbatim.
//
// Not generated from your models: this block is identical in every project
// cyclonec generates for. It is here so the generated tree is self-contained -
// nothing to add to Cargo.toml, nothing to import.
// ==========================================================================

/// A byte stream that does not satisfy the Cyclone Specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum DecodeError {
    /// Fewer bytes remain than the value being read requires, **after the read
    /// had already begun**.
    ///
    /// Bytes running out exactly on a field boundary is not this error - it is
    /// version skew (RFC-0002 §9.1), and the generated decoder handles it
    /// without asking the runtime.
    UnexpectedEof {
        /// Bytes the read needed.
        needed: usize,
        /// Bytes actually left.
        remaining: usize,
    },
    /// A `bool` byte that is neither `0x00` nor `0x01` (RFC-0002 §2.4).
    InvalidBool(u8),
    /// A `string` region that is not valid UTF-8.
    InvalidUtf8,
    /// A length field beyond the configured limit.
    LengthOverflow {
        /// The length read from the stream.
        length: usize,
        /// The limit it exceeded.
        limit: usize,
    },
}

impl ::core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        match *self {
            DecodeError::UnexpectedEof { needed, remaining } => {
                write!(f, "unexpected eof: needed {needed} bytes, {remaining} remaining")
            }
            DecodeError::InvalidBool(byte) => {
                write!(f, "invalid bool: 0x{byte:02X} is neither 0x00 nor 0x01")
            }
            DecodeError::InvalidUtf8 => f.write_str("invalid utf-8 in string"),
            DecodeError::LengthOverflow { length, limit } => {
                write!(f, "length overflow: length {length} exceeds limit {limit}")
            }
        }
    }
}

impl ::std::error::Error for DecodeError {}

/// Allocation guards applied while decoding (RFC-0002 §12).
///
/// A `u32` length can claim up to 4 GiB, so a decoder that allocates straight
/// from an untrusted one is a denial-of-service target. These are **not part of
/// the wire format**: two peers with different limits may disagree about a byte
/// stream, and neither is wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub struct Limits {
    /// Largest accepted UTF-8 byte length of a `string`.
    pub max_string_len: usize,
    /// Largest accepted byte length of a `bytes` blob.
    pub max_bytes_len: usize,
    /// Largest accepted element count of an `Array<T>` (RFC-0002 §6).
    pub max_array_count: usize,
}

#[allow(dead_code)]
impl Limits {
    /// The permissive default: `u32::MAX` for every field.
    pub const UNLIMITED: Limits = Limits {
        max_string_len: u32::MAX as usize,
        max_bytes_len: u32::MAX as usize,
        max_array_count: u32::MAX as usize,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Limits::UNLIMITED
    }
}

/// Appends Cyclone-encoded values to a growable buffer.
///
/// Every multi-byte value is Little Endian, with no padding, no alignment and
/// no metadata between values.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct Writer {
    buf: ::std::vec::Vec<u8>,
}

#[allow(dead_code)]
impl Writer {
    /// Creates an empty writer.
    pub fn new() -> Writer {
        Writer { buf: ::std::vec::Vec::new() }
    }

    /// Creates an empty writer with room for `capacity` bytes.
    pub fn with_capacity(capacity: usize) -> Writer {
        Writer { buf: ::std::vec::Vec::with_capacity(capacity) }
    }

    /// The bytes written so far.
    pub fn as_slice(&self) -> &[u8] {
        &self.buf
    }

    /// Consumes the writer and returns the bytes.
    pub fn into_bytes(self) -> ::std::vec::Vec<u8> {
        self.buf
    }

    /// The number of bytes written so far.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Whether nothing has been written yet.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Discards everything written, keeping the capacity.
    pub fn clear(&mut self) {
        self.buf.clear();
    }

    /// Writes a `bool` as one byte: `0x00` or `0x01`, never anything else.
    pub fn write_bool(&mut self, value: bool) {
        self.buf.push(u8::from(value));
    }

    /// Writes an `i8` as 1 byte.
    pub fn write_i8(&mut self, value: i8) {
        self.buf.push(value as u8);
    }

    /// Writes a `u8` as 1 byte.
    pub fn write_u8(&mut self, value: u8) {
        self.buf.push(value);
    }

    /// Writes an `i16` as 2 bytes, Little Endian.
    pub fn write_i16(&mut self, value: i16) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    /// Writes a `u16` as 2 bytes, Little Endian.
    pub fn write_u16(&mut self, value: u16) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    /// Writes an `i32` as 4 bytes, Little Endian.
    pub fn write_i32(&mut self, value: i32) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    /// Writes a `u32` as 4 bytes, Little Endian.
    pub fn write_u32(&mut self, value: u32) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    /// Writes an `i64` as 8 bytes, Little Endian.
    pub fn write_i64(&mut self, value: i64) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    /// Writes a `u64` as 8 bytes, Little Endian.
    pub fn write_u64(&mut self, value: u64) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    /// Writes an `f32` as its raw IEEE 754 bits, 4 bytes Little Endian.
    ///
    /// The bit pattern is written unmodified: `NaN` payloads survive and `-0.0`
    /// stays distinct from `0.0`.
    pub fn write_f32(&mut self, value: f32) {
        self.buf.extend_from_slice(&value.to_bits().to_le_bytes());
    }

    /// Writes an `f64` as its raw IEEE 754 bits, 8 bytes Little Endian.
    pub fn write_f64(&mut self, value: f64) {
        self.buf.extend_from_slice(&value.to_bits().to_le_bytes());
    }

    /// Writes a `string` as a `u32` UTF-8 **byte** length, then those bytes.
    ///
    /// The length counts bytes, not characters.
    ///
    /// # Panics
    ///
    /// If the string is longer than `u32::MAX` bytes, which the wire format
    /// cannot represent.
    pub fn write_string(&mut self, value: &str) {
        self.write_len(value.len());
        self.buf.extend_from_slice(value.as_bytes());
    }

    /// Writes a `bytes` blob as a `u32` length, then the raw bytes.
    ///
    /// # Panics
    ///
    /// If the blob is longer than `u32::MAX` bytes.
    pub fn write_bytes(&mut self, value: &[u8]) {
        self.write_len(value.len());
        self.buf.extend_from_slice(value);
    }

    /// Writes an `Array<T>`'s element count (RFC-0002 §6) - the caller writes
    /// each element itself, in order, right after.
    ///
    /// # Panics
    ///
    /// If `count` is longer than `u32::MAX`, which the wire format cannot
    /// represent.
    pub fn write_array_count(&mut self, count: usize) {
        self.write_len(count);
    }

    fn write_len(&mut self, len: usize) {
        let len = u32::try_from(len)
            .expect("cyclone: length exceeds u32::MAX and cannot be represented on the wire");
        self.write_u32(len);
    }
}

/// Reads Cyclone-encoded values from a borrowed buffer.
///
/// Malformed input is always a [`DecodeError`], never a panic, and a failed
/// read leaves the cursor where it was.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
    limits: Limits,
}

#[allow(dead_code)]
impl<'a> Reader<'a> {
    /// Creates a reader over `buf` with [`Limits::UNLIMITED`].
    pub fn new(buf: &'a [u8]) -> Reader<'a> {
        Reader { buf, pos: 0, limits: Limits::UNLIMITED }
    }

    /// Creates a reader over `buf` with explicit allocation guards.
    pub fn with_limits(buf: &'a [u8], limits: Limits) -> Reader<'a> {
        Reader { buf, pos: 0, limits }
    }

    /// The cursor position, in bytes from the start.
    pub fn position(&self) -> usize {
        self.pos
    }

    /// The number of bytes left to read.
    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    /// Whether the cursor has reached the end.
    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    /// Whether the field about to be read is **absent** rather than truncated.
    ///
    /// A generated decoder calls this at every field boundary, and it is the
    /// whole of RFC-0002 §9.1's first rule:
    ///
    /// ```text
    /// remaining() == 0 at a field boundary
    ///   → the writer's model stopped here; this field and every field after
    ///     it are absent, and take their zero value. Not an error.
    ///
    /// remaining() > 0 but fewer bytes than the field needs
    ///   → the field started and the stream ran out inside it. That is a
    ///     truncated packet: DecodeError::UnexpectedEof, never a zero.
    /// ```
    ///
    /// The distinction is the reason this method exists. Treating a partial
    /// field as a zero would hide packet corruption behind a plausible value.
    pub fn field_absent(&self) -> bool {
        self.remaining() == 0
    }

    /// The limits this reader enforces.
    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// Reads a `bool` from 1 byte.
    ///
    /// # Errors
    ///
    /// [`DecodeError::InvalidBool`] for any byte but `0x00` and `0x01` -
    /// "non-zero means true" is not permitted.
    pub fn read_bool(&mut self) -> ::core::result::Result<bool, DecodeError> {
        let byte = self.read_u8()?;
        match byte {
            0x00 => ::core::result::Result::Ok(false),
            0x01 => ::core::result::Result::Ok(true),
            other => {
                self.pos -= 1;
                ::core::result::Result::Err(DecodeError::InvalidBool(other))
            }
        }
    }

    /// Reads an `i8` from 1 byte.
    pub fn read_i8(&mut self) -> ::core::result::Result<i8, DecodeError> {
        ::core::result::Result::Ok(self.take(1)?[0] as i8)
    }

    /// Reads a `u8` from 1 byte.
    pub fn read_u8(&mut self) -> ::core::result::Result<u8, DecodeError> {
        ::core::result::Result::Ok(self.take(1)?[0])
    }

    /// Reads an `i16` from 2 bytes, Little Endian.
    pub fn read_i16(&mut self) -> ::core::result::Result<i16, DecodeError> {
        let bytes = self.take(2)?;
        ::core::result::Result::Ok(i16::from_le_bytes([bytes[0], bytes[1]]))
    }

    /// Reads a `u16` from 2 bytes, Little Endian.
    pub fn read_u16(&mut self) -> ::core::result::Result<u16, DecodeError> {
        let bytes = self.take(2)?;
        ::core::result::Result::Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    /// Reads an `i32` from 4 bytes, Little Endian.
    pub fn read_i32(&mut self) -> ::core::result::Result<i32, DecodeError> {
        ::core::result::Result::Ok(self.read_u32()? as i32)
    }

    /// Reads a `u32` from 4 bytes, Little Endian.
    pub fn read_u32(&mut self) -> ::core::result::Result<u32, DecodeError> {
        let bytes = self.take(4)?;
        ::core::result::Result::Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Reads an `i64` from 8 bytes, Little Endian.
    pub fn read_i64(&mut self) -> ::core::result::Result<i64, DecodeError> {
        ::core::result::Result::Ok(self.read_u64()? as i64)
    }

    /// Reads a `u64` from 8 bytes, Little Endian.
    pub fn read_u64(&mut self) -> ::core::result::Result<u64, DecodeError> {
        let bytes = self.take(8)?;
        ::core::result::Result::Ok(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    /// Reads an `f32` from its raw 4-byte IEEE 754 bits.
    ///
    /// The bits are reinterpreted, never normalized.
    pub fn read_f32(&mut self) -> ::core::result::Result<f32, DecodeError> {
        ::core::result::Result::Ok(f32::from_bits(self.read_u32()?))
    }

    /// Reads an `f64` from its raw 8-byte IEEE 754 bits.
    pub fn read_f64(&mut self) -> ::core::result::Result<f64, DecodeError> {
        ::core::result::Result::Ok(f64::from_bits(self.read_u64()?))
    }

    /// Reads a `string`: a `u32` UTF-8 byte length, then that many bytes.
    ///
    /// The length is checked against the limit and against the bytes actually
    /// remaining **before** anything is allocated (RFC-0002 §10.1).
    pub fn read_string(&mut self) -> ::core::result::Result<::std::string::String, DecodeError> {
        let start = self.pos;
        let len = self.read_len(self.limits.max_string_len)?;

        let bytes = match self.take(len) {
            ::core::result::Result::Ok(bytes) => bytes,
            ::core::result::Result::Err(error) => {
                self.pos = start;
                return ::core::result::Result::Err(error);
            }
        };

        match ::core::str::from_utf8(bytes) {
            ::core::result::Result::Ok(text) => {
                ::core::result::Result::Ok(::std::borrow::ToOwned::to_owned(text))
            }
            ::core::result::Result::Err(_) => {
                self.pos = start;
                ::core::result::Result::Err(DecodeError::InvalidUtf8)
            }
        }
    }

    /// Reads a `bytes` blob: a `u32` length, then that many raw bytes.
    pub fn read_bytes(&mut self) -> ::core::result::Result<::std::vec::Vec<u8>, DecodeError> {
        let start = self.pos;
        let len = self.read_len(self.limits.max_bytes_len)?;

        match self.take(len) {
            ::core::result::Result::Ok(bytes) => {
                ::core::result::Result::Ok(::std::vec::Vec::from(bytes))
            }
            ::core::result::Result::Err(error) => {
                self.pos = start;
                ::core::result::Result::Err(error)
            }
        }
    }

    /// Reads an `Array<T>`'s element count (RFC-0002 §6), checked against
    /// [`Limits::max_array_count`] before the caller reads a single element.
    pub fn read_array_count(&mut self) -> ::core::result::Result<usize, DecodeError> {
        self.read_len(self.limits.max_array_count)
    }

    /// Reads a `u32` length prefix and checks it against `limit`.
    fn read_len(&mut self, limit: usize) -> ::core::result::Result<usize, DecodeError> {
        let start = self.pos;
        let len = self.read_u32()? as usize;
        if len > limit {
            self.pos = start;
            return ::core::result::Result::Err(DecodeError::LengthOverflow { length: len, limit });
        }
        ::core::result::Result::Ok(len)
    }

    /// Borrows the next `len` bytes and advances the cursor.
    ///
    /// The single place the remaining-bytes check lives, so no read path can
    /// allocate or index past the end.
    fn take(&mut self, len: usize) -> ::core::result::Result<&'a [u8], DecodeError> {
        let remaining = self.remaining();
        if len > remaining {
            return ::core::result::Result::Err(DecodeError::UnexpectedEof {
                needed: len,
                remaining,
            });
        }
        let bytes = &self.buf[self.pos..self.pos + len];
        self.pos += len;
        ::core::result::Result::Ok(bytes)
    }
}
"####;
