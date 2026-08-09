//! The Cyclone runtime, carried verbatim into every generated GDScript file.
//!
//! The GDScript counterpart of [`super::rust_runtime`], [`super::csharp_runtime`]
//! and [`super::go_runtime`] - same reasoning, same guarantee: the block below
//! is fixed, written once against RFC-0002, and copied out unchanged.
//!
//! # Why `PackedByteArray.encode_u32` / `encode_float` and not hand-rolled bit
//! shifting
//!
//! Every other runtime in this project (Rust's `to_le_bytes`, Go's
//! `encoding/binary.LittleEndian`, C#'s own shift-and-mask) uses an *explicit,
//! contractually Little Endian* primitive - never the host's native byte
//! order. `PackedByteArray`'s `encode_u8`/`encode_u16`/.../`encode_float`/
//! `encode_double` (and the matching `decode_*`) are that primitive for
//! GDScript: their engine implementation (`core/io/marshalls.h`) writes the
//! least-significant byte first, unconditionally, regardless of host CPU byte
//! order - the same explicit-not-native guarantee the other three runtimes
//! rely on - and floats/doubles round-trip their exact IEEE 754 bits through a
//! union reinterpretation, not an arithmetic reconstruction, so a NaN payload
//! and the sign of `-0.0` survive, matching RFC-0002 and every sibling
//! runtime's own documented promise.
//!
//! Two things are worth being honest about, because this is a wire format and
//! not a detail:
//!
//! 1. Godot's own class reference documents this encoding as "an
//!    implementation detail" not to be relied on "when interacting with
//!    external apps" - which is exactly this runtime's use. The engine source
//!    (an explicit byte-shift loop, not a `memcpy`) has not used host-native
//!    order in any Godot 4 release, and changing it would silently break
//!    Godot's own multiplayer RPC/save-game serialization, which is built on
//!    the same primitive - but this is not a documented contract the way
//!    Rust's `to_le_bytes` is, and a future Godot version is the one thing
//!    that could move this out from under a generated file without this
//!    project's own tests (which cannot run a real Godot binary - see the
//!    generator's module docs) ever seeing it move.
//! 2. Hand-rolling the same bit shifts in pure GDScript instead - the way the
//!    other three runtimes do in their own languages - was considered and
//!    rejected: without a Godot interpreter available to run a single line of
//!    this against, untested bit-shift arithmetic (particularly GDScript's
//!    64-bit signed `int` and its arithmetic-shift semantics on negative
//!    values) is a *larger* correctness risk than trusting an engine
//!    primitive that ships with, and is exercised continuously by, every
//!    Godot multiplayer project. See h.md's own priority order - correctness
//!    first - which is why this tradeoff is decided this way and documented
//!    here rather than silently picked.
//!
//! # No exceptions
//!
//! GDScript has no `try`/`catch`. Where Rust returns `Result` and C# throws,
//! decode here returns a `DecodeError` or `null` - the same shape Go's `error`
//! return already gives this project, adapted to a language with no tuple
//! destructuring: every read here returns a two-element `Array` (`[value,
//! error-or-null]`) instead of Go's two-value return, since that is the
//! nearest GDScript equivalent.
//!
//! # Instance methods only, never a cross-class-scope call
//!
//! Every codec, and every runtime type, is an ordinary GDScript class
//! constructed with `.new()`, and every `DecodeError` is built inline, right
//! where it is needed, rather than through a shared helper function. Two
//! uncertainties - neither checkable without a real Godot binary, see the
//! generator's module docs - are avoided on purpose by that choice: whether a
//! `static func` is well-formed *inside* a nested class (only "consistent
//! with the language's general rules," never shown as a combined example),
//! and whether an inner class's method can call a bare unqualified helper
//! declared on the file's own outer wrapper class. Skipping both questions
//! entirely costs a few repeated lines; guessing wrong on either would cost a
//! file that does not compile.

/// The runtime block, emitted once at the top of every generated GDScript
/// file, at column 0 alongside the file's own `class_name` wrapper (see
/// [`super::gdscript::WRAPPER_CLASS_NAME`] - `class_name` does not open an
/// indented block, so these are sibling declarations, not nested ones) so
/// every type here is reachable project-wide as `CycloneCodec.Writer`,
/// `CycloneCodec.Reader`, ... with nothing to `preload` or `import` -
/// GDScript's own counterpart of Rust's `include!`, C#'s "just add the file
/// to the project", and Go's "same package".
pub const RUNTIME: &str = r#"
# ======================================================================
# Cyclone runtime - RFC-0002, carried verbatim.
#
# Not generated from your models: this block is identical in every file
# cyclonec writes. It is here so the file is self-contained.
# ======================================================================

# A byte stream that does not satisfy the Cyclone Specification.
#
# GDScript has no exceptions, so every `decode` below returns a
# DecodeError (or null, on success) instead of throwing one - see this
# module's file-level docs.
class DecodeError:
	var kind: String = ""
	var needed: int = 0
	var remaining: int = 0
	var invalid_byte: int = 0
	var length: int = 0
	var limit: int = 0

	func message() -> String:
		match kind:
			"unexpected_eof":
				return "unexpected eof: needed %d bytes, %d remaining" % [needed, remaining]
			"invalid_bool":
				return "invalid bool: 0x%02X is neither 0x00 nor 0x01" % invalid_byte
			"invalid_utf8":
				return "invalid utf-8 in string"
			"length_overflow":
				return "length overflow: length %d exceeds limit %d" % [length, limit]
			_:
				return "cyclone: decode error"

# Allocation guards applied while decoding (RFC-0002 §4).
#
# A u32 length can claim up to 4 GiB, so a decoder that allocates straight
# from an untrusted one is a denial-of-service target. These are not part
# of the wire format: two peers with different limits may disagree about a
# byte stream, and neither is wrong.
class Limits:
	var max_string_len: int = 0xFFFFFFFF
	var max_bytes_len: int = 0xFFFFFFFF
	# Largest accepted element count of an Array<T> (RFC-0002 §6). A u32
	# count can claim up to 4 GiB of elements before a single one is even
	# read, so this guards allocation the same way max_string_len/
	# max_bytes_len do.
	var max_array_count: int = 0xFFFFFFFF

# Writer appends Cyclone-encoded values to a growable buffer.
#
# Every multi-byte value is Little Endian, with no padding, no alignment
# and no metadata between values.
class Writer:
	var buf: PackedByteArray = PackedByteArray()

	func bytes() -> PackedByteArray:
		return buf

	func size() -> int:
		return buf.size()

	func write_bool(value: bool) -> void:
		write_u8(1 if value else 0)

	func write_i8(value: int) -> void:
		var start := buf.size()
		buf.resize(start + 1)
		buf.encode_s8(start, value)

	func write_u8(value: int) -> void:
		var start := buf.size()
		buf.resize(start + 1)
		buf.encode_u8(start, value)

	func write_i16(value: int) -> void:
		var start := buf.size()
		buf.resize(start + 2)
		buf.encode_s16(start, value)

	func write_u16(value: int) -> void:
		var start := buf.size()
		buf.resize(start + 2)
		buf.encode_u16(start, value)

	func write_i32(value: int) -> void:
		var start := buf.size()
		buf.resize(start + 4)
		buf.encode_s32(start, value)

	func write_u32(value: int) -> void:
		var start := buf.size()
		buf.resize(start + 4)
		buf.encode_u32(start, value)

	# u64/i64 share one 64-bit two's-complement word either way: GDScript's
	# `int` is itself a 64-bit signed value, so a `u64` field whose top bit
	# is set reads back as a negative GDScript `int` - the wire bytes are
	# still exactly right (h.md's own rule: no native type decides the
	# wire format). Interpreting that bit pattern as unsigned, if a caller
	# needs to, is the caller's business, the same way this project leaves
	# an `[Network("u32")]` on a C# `ulong` to the C# compiler.
	func write_i64(value: int) -> void:
		var start := buf.size()
		buf.resize(start + 8)
		buf.encode_s64(start, value)

	func write_u64(value: int) -> void:
		var start := buf.size()
		buf.resize(start + 8)
		buf.encode_u64(start, value)

	# Raw IEEE 754 bits, 4 bytes Little Endian - not normalized: NaN
	# payloads survive and -0.0 stays distinct from 0.0.
	func write_f32(value: float) -> void:
		var start := buf.size()
		buf.resize(start + 4)
		buf.encode_float(start, value)

	func write_f64(value: float) -> void:
		var start := buf.size()
		buf.resize(start + 8)
		buf.encode_double(start, value)

	# A uint32 UTF-8 byte length, then those bytes. The length counts
	# bytes, not characters.
	func write_string(value: String) -> void:
		var utf8 := value.to_utf8_buffer()
		write_u32(utf8.size())
		buf.append_array(utf8)

	# A uint32 length, then the raw bytes.
	func write_bytes(value: PackedByteArray) -> void:
		write_u32(value.size())
		buf.append_array(value)

	# Writes an Array<T>'s element count (RFC-0002 §6) - the caller writes
	# each element itself, in order, right after.
	func write_array_count(count: int) -> void:
		write_u32(count)

# Reader reads Cyclone-encoded values from a borrowed buffer.
#
# Malformed input is always a DecodeError, returned - never thrown, GDScript
# has no exceptions - and a failed read leaves the cursor where it was.
#
# Every read below returns a 2-element Array, `[value, error]`, with
# `error` null on success - the nearest GDScript has to Go's `(value,
# err)`, since GDScript cannot destructure a multi-value return.
class Reader:
	var buf: PackedByteArray
	var pos: int = 0
	var limits: Limits

	func _init(bytes: PackedByteArray, with_limits: Limits = null) -> void:
		buf = bytes
		pos = 0
		limits = with_limits if with_limits != null else Limits.new()

	func position() -> int:
		return pos

	func remaining() -> int:
		return buf.size() - pos

	func is_empty() -> bool:
		return remaining() == 0

	func read_bool() -> Array:
		var result := read_u8()
		if result[1] != null:
			return [false, result[1]]
		var value: int = result[0]
		if value == 0:
			return [false, null]
		if value == 1:
			return [true, null]
		pos -= 1
		var error := DecodeError.new()
		error.kind = "invalid_bool"
		error.invalid_byte = value
		return [false, error]

	func read_i8() -> Array:
		var taken := _take(1)
		if taken[1] != null:
			return [0, taken[1]]
		var bytes: PackedByteArray = taken[0]
		return [bytes.decode_s8(0), null]

	func read_u8() -> Array:
		var taken := _take(1)
		if taken[1] != null:
			return [0, taken[1]]
		var bytes: PackedByteArray = taken[0]
		return [bytes.decode_u8(0), null]

	func read_i16() -> Array:
		var taken := _take(2)
		if taken[1] != null:
			return [0, taken[1]]
		var bytes: PackedByteArray = taken[0]
		return [bytes.decode_s16(0), null]

	func read_u16() -> Array:
		var taken := _take(2)
		if taken[1] != null:
			return [0, taken[1]]
		var bytes: PackedByteArray = taken[0]
		return [bytes.decode_u16(0), null]

	func read_i32() -> Array:
		var taken := _take(4)
		if taken[1] != null:
			return [0, taken[1]]
		var bytes: PackedByteArray = taken[0]
		return [bytes.decode_s32(0), null]

	func read_u32() -> Array:
		var taken := _take(4)
		if taken[1] != null:
			return [0, taken[1]]
		var bytes: PackedByteArray = taken[0]
		return [bytes.decode_u32(0), null]

	func read_i64() -> Array:
		var taken := _take(8)
		if taken[1] != null:
			return [0, taken[1]]
		var bytes: PackedByteArray = taken[0]
		return [bytes.decode_s64(0), null]

	func read_u64() -> Array:
		var taken := _take(8)
		if taken[1] != null:
			return [0, taken[1]]
		var bytes: PackedByteArray = taken[0]
		return [bytes.decode_u64(0), null]

	func read_f32() -> Array:
		var taken := _take(4)
		if taken[1] != null:
			return [0.0, taken[1]]
		var bytes: PackedByteArray = taken[0]
		return [bytes.decode_float(0), null]

	func read_f64() -> Array:
		var taken := _take(8)
		if taken[1] != null:
			return [0.0, taken[1]]
		var bytes: PackedByteArray = taken[0]
		return [bytes.decode_double(0), null]

	# A uint32 UTF-8 byte length, then that many bytes. The length is
	# checked against the limit and against the bytes actually remaining
	# before anything is allocated.
	func read_string() -> Array:
		var start := pos
		var length_result := _read_length(limits.max_string_len)
		if length_result[1] != null:
			return ["", length_result[1]]
		var length: int = length_result[0]

		var taken := _take_checked(length, start)
		if taken[1] != null:
			return ["", taken[1]]
		var bytes: PackedByteArray = taken[0]

		var text := bytes.get_string_from_utf8()
		if text == "" and length != 0:
			pos = start
			var error := DecodeError.new()
			error.kind = "invalid_utf8"
			return ["", error]
		return [text, null]

	# A uint32 length, then that many raw bytes.
	func read_bytes() -> Array:
		var start := pos
		var length_result := _read_length(limits.max_bytes_len)
		if length_result[1] != null:
			return [PackedByteArray(), length_result[1]]
		var length: int = length_result[0]
		return _take_checked(length, start)

	# Reads an Array<T>'s element count (RFC-0002 §6), checked against
	# limits.max_array_count before the caller reads a single element -
	# the same allocation guard read_string/read_bytes apply to their own
	# length prefix. Returns [count, error], the same two-slot convention
	# every read here follows.
	func read_array_count() -> Array:
		return _read_length(limits.max_array_count)

	func _read_length(limit: int) -> Array:
		var start := pos
		var result := read_u32()
		if result[1] != null:
			return [0, result[1]]
		var length: int = result[0]
		if length > limit:
			pos = start
			var error := DecodeError.new()
			error.kind = "length_overflow"
			error.length = length
			error.limit = limit
			return [0, error]
		return [length, null]

	func _take_checked(length: int, start: int) -> Array:
		var left := remaining()
		if length > left:
			pos = start
			var error := DecodeError.new()
			error.kind = "unexpected_eof"
			error.needed = length
			error.remaining = left
			return [PackedByteArray(), error]
		return _take(length)

	# Borrows the next `length` bytes and advances the cursor - the single
	# place the remaining-bytes check lives, so no read path can slice past
	# the end.
	func _take(length: int) -> Array:
		var left := remaining()
		if length > left:
			var error := DecodeError.new()
			error.kind = "unexpected_eof"
			error.needed = length
			error.remaining = left
			return [PackedByteArray(), error]
		var out := buf.slice(pos, pos + length)
		pos += length
		return [out, null]
"#;
