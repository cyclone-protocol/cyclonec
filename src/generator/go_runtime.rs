//! The Cyclone runtime, carried verbatim into `runtime.go`.
//!
//! The Go counterpart of [`super::rust_runtime`] - same reasoning, same
//! guarantee: the block below is fixed, written once against RFC-0002, and
//! copied out unchanged. Nothing about byte layout is computed per model, per
//! field, or per run.
//!
//! Go has no exceptions, so where Rust returns `Result`, `Reader`'s methods
//! return `(T, error)` - the idiom every generated `Decode` is written
//! against: `value.ID, err = r.ReadU32()`.
//!
//! # What changed from `cyclonec_old`
//!
//! One method: [`Reader::FieldAbsent`] (see the block below). The old runtime
//! gave a generated decoder no way to tell *this field never arrived* from
//! *this field arrived truncated* - every read simply returned an
//! `unexpected_eof` `DecodeError` - so the decoder could not implement
//! RFC-0002 §9.1 at all. See `generator::go` for what the generated decoder
//! does with it; the fix is identical in spirit to [`super::rust_runtime`]'s.

/// The runtime block, emitted once, into its own file, right after the
/// package clause.
pub const RUNTIME: &str = r####"
// ==========================================================================
// Cyclone runtime - RFC-0002, carried verbatim.
//
// Not generated from your models: this block is identical in every project
// cyclonec generates for. It is here so the generated package is
// self-contained - nothing to add to go.mod, nothing to import.
// ==========================================================================

import (
	"encoding/binary"
	"fmt"
	"math"
	"unicode/utf8"
)

// DecodeError is a byte stream that does not satisfy the Cyclone
// Specification.
type DecodeError struct {
	// Kind names which Specification error condition this is.
	Kind string
	// Needed is the number of bytes a read needed. Set for "unexpected_eof".
	Needed int
	// Remaining is the number of bytes actually left. Set for "unexpected_eof".
	Remaining int
	// InvalidByte is the offending byte. Set for "invalid_bool".
	InvalidByte byte
	// Length is the length or count read from the stream. Set for
	// "length_overflow".
	Length int
	// Limit is the configured limit it exceeded. Set for "length_overflow".
	Limit int
}

func (e *DecodeError) Error() string {
	switch e.Kind {
	case "unexpected_eof":
		return fmt.Sprintf("unexpected eof: needed %d bytes, %d remaining", e.Needed, e.Remaining)
	case "invalid_bool":
		return fmt.Sprintf("invalid bool: 0x%02X is neither 0x00 nor 0x01", e.InvalidByte)
	case "invalid_utf8":
		return "invalid utf-8 in string"
	case "length_overflow":
		return fmt.Sprintf("length overflow: length %d exceeds limit %d", e.Length, e.Limit)
	default:
		return "cyclone: decode error"
	}
}

func errUnexpectedEOF(needed, remaining int) error {
	return &DecodeError{Kind: "unexpected_eof", Needed: needed, Remaining: remaining}
}

func errInvalidBool(value byte) error {
	return &DecodeError{Kind: "invalid_bool", InvalidByte: value}
}

func errInvalidUTF8() error {
	return &DecodeError{Kind: "invalid_utf8"}
}

func errLengthOverflow(length, limit int) error {
	return &DecodeError{Kind: "length_overflow", Length: length, Limit: limit}
}

// Limits are allocation guards applied while decoding (RFC-0002 §12).
//
// A uint32 length can claim up to 4 GiB, so a decoder that allocates straight
// from an untrusted one is a denial-of-service target. These are not part of
// the wire format: two peers with different limits may disagree about a byte
// stream, and neither is wrong.
type Limits struct {
	// MaxStringLen is the largest accepted UTF-8 byte length of a string.
	MaxStringLen int
	// MaxBytesLen is the largest accepted byte length of a bytes blob.
	MaxBytesLen int
	// MaxArrayCount is the largest accepted element count of an Array<T>
	// (RFC-0002 §6). A uint32 count can claim up to 4 GiB of elements
	// before a single one is even read, so this guards allocation the
	// same way MaxStringLen/MaxBytesLen do.
	MaxArrayCount int
}

// UnlimitedLimits is the permissive default: math.MaxUint32 for every field.
var UnlimitedLimits = Limits{
	MaxStringLen:  math.MaxUint32,
	MaxBytesLen:   math.MaxUint32,
	MaxArrayCount: math.MaxUint32,
}

// Writer appends Cyclone-encoded values to a growable buffer.
//
// Every multi-byte value is Little Endian, with no padding, no alignment and
// no metadata between values.
type Writer struct {
	buf []byte
}

// NewWriter creates an empty writer.
func NewWriter() *Writer {
	return &Writer{}
}

// Bytes returns the bytes written so far.
func (w *Writer) Bytes() []byte {
	if w.buf == nil {
		return []byte{}
	}
	return w.buf
}

// Len returns the number of bytes written so far.
func (w *Writer) Len() int {
	return len(w.buf)
}

// WriteBool writes a bool as one byte: 0x00 or 0x01, never anything else.
func (w *Writer) WriteBool(value bool) {
	if value {
		w.buf = append(w.buf, 0x01)
	} else {
		w.buf = append(w.buf, 0x00)
	}
}

// WriteI8 writes an int8 as 1 byte.
func (w *Writer) WriteI8(value int8) {
	w.buf = append(w.buf, byte(value))
}

// WriteU8 writes a uint8 as 1 byte.
func (w *Writer) WriteU8(value uint8) {
	w.buf = append(w.buf, value)
}

// WriteI16 writes an int16 as 2 bytes, Little Endian.
func (w *Writer) WriteI16(value int16) {
	w.WriteU16(uint16(value))
}

// WriteU16 writes a uint16 as 2 bytes, Little Endian.
func (w *Writer) WriteU16(value uint16) {
	var tmp [2]byte
	binary.LittleEndian.PutUint16(tmp[:], value)
	w.buf = append(w.buf, tmp[:]...)
}

// WriteI32 writes an int32 as 4 bytes, Little Endian.
func (w *Writer) WriteI32(value int32) {
	w.WriteU32(uint32(value))
}

// WriteU32 writes a uint32 as 4 bytes, Little Endian.
func (w *Writer) WriteU32(value uint32) {
	var tmp [4]byte
	binary.LittleEndian.PutUint32(tmp[:], value)
	w.buf = append(w.buf, tmp[:]...)
}

// WriteI64 writes an int64 as 8 bytes, Little Endian.
func (w *Writer) WriteI64(value int64) {
	w.WriteU64(uint64(value))
}

// WriteU64 writes a uint64 as 8 bytes, Little Endian.
func (w *Writer) WriteU64(value uint64) {
	var tmp [8]byte
	binary.LittleEndian.PutUint64(tmp[:], value)
	w.buf = append(w.buf, tmp[:]...)
}

// WriteF32 writes a float32 as its raw IEEE 754 bits, 4 bytes Little Endian.
//
// The bit pattern is written unmodified: NaN payloads survive and -0.0 stays
// distinct from 0.0.
func (w *Writer) WriteF32(value float32) {
	w.WriteU32(math.Float32bits(value))
}

// WriteF64 writes a float64 as its raw IEEE 754 bits, 8 bytes Little Endian.
func (w *Writer) WriteF64(value float64) {
	w.WriteU64(math.Float64bits(value))
}

// WriteString writes a string as a uint32 UTF-8 byte length, then those bytes.
//
// The length counts bytes, not characters.
func (w *Writer) WriteString(value string) {
	w.WriteU32(uint32(len(value)))
	w.buf = append(w.buf, value...)
}

// WriteBytes writes a bytes blob as a uint32 length, then the raw bytes.
func (w *Writer) WriteBytes(value []byte) {
	w.WriteU32(uint32(len(value)))
	w.buf = append(w.buf, value...)
}

// WriteArrayCount writes an Array<T>'s element count (RFC-0002 §6) - the
// caller writes each element itself, in order, right after.
func (w *Writer) WriteArrayCount(count int) {
	w.WriteU32(uint32(count))
}

// Reader reads Cyclone-encoded values from a borrowed buffer.
//
// Malformed input is always a *DecodeError, never a panic, and a failed read
// leaves the cursor where it was.
type Reader struct {
	buf    []byte
	pos    int
	limits Limits
}

// NewReader creates a reader over buf with the permissive UnlimitedLimits.
func NewReader(buf []byte) *Reader {
	return &Reader{buf: buf, limits: UnlimitedLimits}
}

// NewReaderWithLimits creates a reader over buf with explicit allocation
// guards.
func NewReaderWithLimits(buf []byte, limits Limits) *Reader {
	return &Reader{buf: buf, limits: limits}
}

// Position returns the cursor position, in bytes from the start.
func (r *Reader) Position() int {
	return r.pos
}

// Remaining returns the number of bytes left to read.
func (r *Reader) Remaining() int {
	return len(r.buf) - r.pos
}

// IsEmpty reports whether the cursor has reached the end.
func (r *Reader) IsEmpty() bool {
	return r.Remaining() == 0
}

// FieldAbsent reports whether the field about to be read is absent rather
// than truncated.
//
// A generated decoder calls this at every field boundary, and it is the whole
// of RFC-0002 §9.1's first rule:
//
//	Remaining() == 0 at a field boundary
//	  -> the writer's model stopped here; this field and every field after
//	     it are absent, and take their zero value. Not an error.
//
//	Remaining() > 0 but fewer bytes than the field needs
//	  -> the field started and the stream ran out inside it. That is a
//	     truncated packet: DecodeError{Kind: "unexpected_eof"}, never a zero.
//
// The distinction is the reason this method exists. Treating a partial field
// as a zero would hide packet corruption behind a plausible value.
func (r *Reader) FieldAbsent() bool {
	return r.Remaining() == 0
}

// Limits returns the limits this reader enforces.
func (r *Reader) Limits() Limits {
	return r.limits
}

// ReadBool reads a bool from 1 byte.
//
// Returns InvalidBool for any byte but 0x00 and 0x01 - "non-zero means true"
// is not permitted.
func (r *Reader) ReadBool() (bool, error) {
	value, err := r.ReadU8()
	if err != nil {
		return false, err
	}
	switch value {
	case 0x00:
		return false, nil
	case 0x01:
		return true, nil
	default:
		r.pos--
		return false, errInvalidBool(value)
	}
}

// ReadI8 reads an int8 from 1 byte.
func (r *Reader) ReadI8() (int8, error) {
	value, err := r.ReadU8()
	return int8(value), err
}

// ReadU8 reads a uint8 from 1 byte.
func (r *Reader) ReadU8() (uint8, error) {
	bytes, err := r.take(1)
	if err != nil {
		return 0, err
	}
	return bytes[0], nil
}

// ReadI16 reads an int16 from 2 bytes, Little Endian.
func (r *Reader) ReadI16() (int16, error) {
	value, err := r.ReadU16()
	return int16(value), err
}

// ReadU16 reads a uint16 from 2 bytes, Little Endian.
func (r *Reader) ReadU16() (uint16, error) {
	bytes, err := r.take(2)
	if err != nil {
		return 0, err
	}
	return binary.LittleEndian.Uint16(bytes), nil
}

// ReadI32 reads an int32 from 4 bytes, Little Endian.
func (r *Reader) ReadI32() (int32, error) {
	value, err := r.ReadU32()
	return int32(value), err
}

// ReadU32 reads a uint32 from 4 bytes, Little Endian.
func (r *Reader) ReadU32() (uint32, error) {
	bytes, err := r.take(4)
	if err != nil {
		return 0, err
	}
	return binary.LittleEndian.Uint32(bytes), nil
}

// ReadI64 reads an int64 from 8 bytes, Little Endian.
func (r *Reader) ReadI64() (int64, error) {
	value, err := r.ReadU64()
	return int64(value), err
}

// ReadU64 reads a uint64 from 8 bytes, Little Endian.
func (r *Reader) ReadU64() (uint64, error) {
	bytes, err := r.take(8)
	if err != nil {
		return 0, err
	}
	return binary.LittleEndian.Uint64(bytes), nil
}

// ReadF32 reads a float32 from its raw 4-byte IEEE 754 bits.
//
// The bits are reinterpreted, never normalized.
func (r *Reader) ReadF32() (float32, error) {
	bits, err := r.ReadU32()
	if err != nil {
		return 0, err
	}
	return math.Float32frombits(bits), nil
}

// ReadF64 reads a float64 from its raw 8-byte IEEE 754 bits.
func (r *Reader) ReadF64() (float64, error) {
	bits, err := r.ReadU64()
	if err != nil {
		return 0, err
	}
	return math.Float64frombits(bits), nil
}

// ReadString reads a string: a uint32 UTF-8 byte length, then that many
// bytes.
//
// The length is checked against the limit and against the bytes actually
// remaining before anything is allocated.
func (r *Reader) ReadString() (string, error) {
	start := r.pos
	length, err := r.readLength(r.limits.MaxStringLen)
	if err != nil {
		return "", err
	}

	bytes, err := r.takeChecked(length, start)
	if err != nil {
		return "", err
	}

	if !utf8.Valid(bytes) {
		r.pos = start
		return "", errInvalidUTF8()
	}
	return string(bytes), nil
}

// ReadBytes reads a bytes blob: a uint32 length, then that many raw bytes.
func (r *Reader) ReadBytes() ([]byte, error) {
	start := r.pos
	length, err := r.readLength(r.limits.MaxBytesLen)
	if err != nil {
		return nil, err
	}

	bytes, err := r.takeChecked(length, start)
	if err != nil {
		return nil, err
	}
	out := make([]byte, len(bytes))
	copy(out, bytes)
	return out, nil
}

// ReadArrayCount reads an Array<T>'s element count (RFC-0002 §6), checked
// against Limits.MaxArrayCount before the caller reads a single element -
// the same allocation guard ReadString and ReadBytes apply to their own
// length prefix.
func (r *Reader) ReadArrayCount() (int, error) {
	return r.readLength(r.limits.MaxArrayCount)
}

func (r *Reader) readLength(limit int) (int, error) {
	start := r.pos
	length, err := r.ReadU32()
	if err != nil {
		return 0, err
	}
	if int(length) > limit {
		r.pos = start
		return 0, errLengthOverflow(int(length), limit)
	}
	return int(length), nil
}

func (r *Reader) takeChecked(length, start int) ([]byte, error) {
	remaining := r.Remaining()
	if length > remaining {
		r.pos = start
		return nil, errUnexpectedEOF(length, remaining)
	}
	return r.take(length)
}

// take borrows the next length bytes and advances the cursor.
//
// The single place the remaining-bytes check lives, so no read path can
// allocate or index past the end.
func (r *Reader) take(length int) ([]byte, error) {
	remaining := r.Remaining()
	if length > remaining {
		return nil, errUnexpectedEOF(length, remaining)
	}
	bytes := r.buf[r.pos : r.pos+length]
	r.pos += length
	return bytes, nil
}
"####;
