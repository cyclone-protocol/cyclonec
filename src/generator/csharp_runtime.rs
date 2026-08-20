pub const RUNTIME: &str = r####"
// ==========================================================================
// Cyclone runtime - RFC-0002, carried verbatim.
//
// Not generated from your models: this block is identical in every project
// cyclonec generates for. It is here so the generated namespace is
// self-contained - nothing to add to your .csproj, nothing to import.
// ==========================================================================

/// A byte stream that does not satisfy the Cyclone Specification.
public sealed class DecodeException : System.Exception
{
    private DecodeException(string message) : base(message) { }

    /// Fewer bytes remain than the value being read requires, **after the
    /// read had already begun**.
    ///
    /// Bytes running out exactly on a field boundary is not this error - it
    /// is version skew (RFC-0002 §9.1), and the generated decoder handles it
    /// without asking the runtime.
    public static DecodeException UnexpectedEof(long needed, long remaining) =>
        new DecodeException(
            $"unexpected eof: needed {needed} bytes, {remaining} remaining");

    /// A `bool` byte that is neither `0x00` nor `0x01` (RFC-0002 §2.4).
    public static DecodeException InvalidBool(byte value) =>
        new DecodeException($"invalid bool: 0x{value:X2} is neither 0x00 nor 0x01");

    /// A `string` region that is not valid UTF-8.
    public static DecodeException InvalidUtf8() =>
        new DecodeException("invalid utf-8 in string");

    /// A length field beyond the configured limit.
    public static DecodeException LengthOverflow(long length, long limit) =>
        new DecodeException($"length overflow: length {length} exceeds limit {limit}");
}

/// Allocation guards applied while decoding (RFC-0002 §12).
///
/// A `u32` length can claim up to 4 GiB, so a decoder that allocates straight
/// from an untrusted one is a denial-of-service target. These are **not part
/// of the wire format**: two peers with different limits may disagree about a
/// byte stream, and neither is wrong.
public struct Limits
{
    /// Largest accepted UTF-8 byte length of a `string`.
    public long MaxStringLength;

    /// Largest accepted byte length of a `bytes` blob.
    public long MaxBytesLength;

    /// Largest accepted element count of an `Array<T>` (RFC-0002 §6). A
    /// `u32` count can claim up to 4 GiB of elements before a single one is
    /// even read, so this guards allocation the same way the string/bytes
    /// limits do.
    public long MaxArrayCount;

    /// The permissive default: `uint.MaxValue` for every field.
    public static Limits Unlimited => new Limits
    {
        MaxStringLength = uint.MaxValue,
        MaxBytesLength = uint.MaxValue,
        MaxArrayCount = uint.MaxValue,
    };
}

/// Appends Cyclone-encoded values to a growable buffer.
///
/// Every multi-byte value is Little Endian, with no padding, no alignment and
/// no metadata between values.
public sealed class Writer
{
    private readonly System.Collections.Generic.List<byte> _buffer =
        new System.Collections.Generic.List<byte>();

    /// The bytes written so far.
    public byte[] ToArray() => _buffer.ToArray();

    /// The number of bytes written so far.
    public int Length => _buffer.Count;

    /// Writes a `bool` as one byte: `0x00` or `0x01`, never anything else.
    public void WriteBool(bool value) => _buffer.Add(value ? (byte)0x01 : (byte)0x00);

    /// Writes an `i8` as 1 byte.
    public void WriteI8(sbyte value) => _buffer.Add(unchecked((byte)value));

    /// Writes a `u8` as 1 byte.
    public void WriteU8(byte value) => _buffer.Add(value);

    /// Writes an `i16` as 2 bytes, Little Endian.
    public void WriteI16(short value) => WriteLittleEndian(unchecked((ushort)value), 2);

    /// Writes a `u16` as 2 bytes, Little Endian.
    public void WriteU16(ushort value) => WriteLittleEndian(value, 2);

    /// Writes an `i32` as 4 bytes, Little Endian.
    public void WriteI32(int value) => WriteLittleEndian(unchecked((uint)value), 4);

    /// Writes a `u32` as 4 bytes, Little Endian.
    public void WriteU32(uint value) => WriteLittleEndian(value, 4);

    /// Writes an `i64` as 8 bytes, Little Endian.
    public void WriteI64(long value) => WriteLittleEndian(unchecked((ulong)value), 8);

    /// Writes a `u64` as 8 bytes, Little Endian.
    public void WriteU64(ulong value) => WriteLittleEndian(value, 8);

    /// Writes an `f32` as its raw IEEE 754 bits, 4 bytes Little Endian.
    ///
    /// The bit pattern is written unmodified: `NaN` payloads survive and
    /// `-0.0` stays distinct from `0.0`.
    public void WriteF32(float value) =>
        WriteLittleEndian(System.BitConverter.SingleToUInt32Bits(value), 4);

    /// Writes an `f64` as its raw IEEE 754 bits, 8 bytes Little Endian.
    public void WriteF64(double value) =>
        WriteLittleEndian(System.BitConverter.DoubleToUInt64Bits(value), 8);

    /// Writes a `string` as a `u32` UTF-8 **byte** length, then those bytes.
    ///
    /// The length counts bytes, not characters.
    public void WriteString(string value)
    {
        byte[] bytes = System.Text.Encoding.UTF8.GetBytes(value);
        WriteU32((uint)bytes.Length);
        _buffer.AddRange(bytes);
    }

    /// Writes a `bytes` blob as a `u32` length, then the raw bytes.
    public void WriteBytes(byte[] value)
    {
        WriteU32((uint)value.Length);
        _buffer.AddRange(value);
    }

    /// Writes an `Array<T>`'s element count (RFC-0002 §6) - the caller
    /// writes each element itself, in order, right after.
    public void WriteArrayCount(int count) => WriteU32((uint)count);

    private void WriteLittleEndian(ulong value, int byteCount)
    {
        for (int i = 0; i < byteCount; i++)
        {
            _buffer.Add((byte)(value >> (8 * i)));
        }
    }
}

/// Reads Cyclone-encoded values from a borrowed buffer.
///
/// Malformed input is always a <see cref="DecodeException"/>, never a silent
/// wrong answer, and a failed read leaves the cursor where it was.
public ref struct Reader
{
    private readonly System.ReadOnlySpan<byte> _buffer;
    private int _position;
    private readonly Limits _limits;

    /// Creates a reader over `buffer` with <see cref="Limits.Unlimited"/>.
    public Reader(System.ReadOnlySpan<byte> buffer) : this(buffer, Limits.Unlimited) { }

    /// Creates a reader over `buffer` with explicit allocation guards.
    public Reader(System.ReadOnlySpan<byte> buffer, Limits limits)
    {
        _buffer = buffer;
        _position = 0;
        _limits = limits;
    }

    /// The cursor position, in bytes from the start.
    public int Position => _position;

    /// The number of bytes left to read.
    public int Remaining => _buffer.Length - _position;

    /// Whether the cursor has reached the end.
    public bool IsEmpty => Remaining == 0;

    /// Whether the field about to be read is **absent** rather than
    /// truncated.
    ///
    /// A generated decoder calls this at every field boundary, and it is the
    /// whole of RFC-0002 §9.1's first rule:
    ///
    ///   Remaining == 0 at a field boundary
    ///     -&gt; the writer's model stopped here; this field and every field
    ///        after it are absent, and take their zero value. Not an error.
    ///
    ///   Remaining &gt; 0 but fewer bytes than the field needs
    ///     -&gt; the field started and the stream ran out inside it. That is
    ///        a truncated packet: <see cref="DecodeException"/>, never a
    ///        zero.
    ///
    /// The distinction is the reason this method exists. Treating a partial
    /// field as a zero would hide packet corruption behind a plausible
    /// value.
    public bool FieldAbsent() => Remaining == 0;

    /// The limits this reader enforces.
    public Limits GetLimits() => _limits;

    /// Reads a `bool` from 1 byte.
    ///
    /// Throws <see cref="DecodeException"/> for any byte but `0x00` and
    /// `0x01` - "non-zero means true" is not permitted.
    public bool ReadBool()
    {
        byte value = ReadU8();
        if (value == 0x00) return false;
        if (value == 0x01) return true;
        _position -= 1;
        throw DecodeException.InvalidBool(value);
    }

    /// Reads an `i8` from 1 byte.
    public sbyte ReadI8() => unchecked((sbyte)Take(1)[0]);

    /// Reads a `u8` from 1 byte.
    public byte ReadU8() => Take(1)[0];

    /// Reads an `i16` from 2 bytes, Little Endian.
    public short ReadI16() => unchecked((short)ReadLittleEndian(2));

    /// Reads a `u16` from 2 bytes, Little Endian.
    public ushort ReadU16() => (ushort)ReadLittleEndian(2);

    /// Reads an `i32` from 4 bytes, Little Endian.
    public int ReadI32() => unchecked((int)ReadLittleEndian(4));

    /// Reads a `u32` from 4 bytes, Little Endian.
    public uint ReadU32() => (uint)ReadLittleEndian(4);

    /// Reads an `i64` from 8 bytes, Little Endian.
    public long ReadI64() => unchecked((long)ReadLittleEndian(8));

    /// Reads a `u64` from 8 bytes, Little Endian.
    public ulong ReadU64() => ReadLittleEndian(8);

    /// Reads an `f32` from its raw 4-byte IEEE 754 bits.
    ///
    /// The bits are reinterpreted, never normalized.
    public float ReadF32() => System.BitConverter.UInt32BitsToSingle(ReadU32());

    /// Reads an `f64` from its raw 8-byte IEEE 754 bits.
    public double ReadF64() => System.BitConverter.UInt64BitsToDouble(ReadU64());

    /// Reads a `string`: a `u32` UTF-8 byte length, then that many bytes.
    ///
    /// The length is checked against the limit and against the bytes
    /// actually remaining **before** anything is allocated.
    public string ReadString()
    {
        int start = _position;
        int len = ReadLength(_limits.MaxStringLength);
        System.ReadOnlySpan<byte> bytes = TakeChecked(len, start);

        try
        {
            return new System.Text.UTF8Encoding(false, true).GetString(bytes);
        }
        catch (System.Text.DecoderFallbackException)
        {
            _position = start;
            throw DecodeException.InvalidUtf8();
        }
    }

    /// Reads a `bytes` blob: a `u32` length, then that many raw bytes.
    public byte[] ReadBytes()
    {
        int start = _position;
        int len = ReadLength(_limits.MaxBytesLength);
        return TakeChecked(len, start).ToArray();
    }

    /// Reads an `Array<T>`'s element count (RFC-0002 §6), checked against
    /// <see cref="Limits.MaxArrayCount"/> before the caller reads a single
    /// element - the same allocation guard <see cref="ReadString"/> and
    /// <see cref="ReadBytes"/> apply to their own length prefix.
    public int ReadArrayCount() => ReadLength(_limits.MaxArrayCount);

    private int ReadLength(long limit)
    {
        int start = _position;
        uint len = ReadU32();
        if (len > limit)
        {
            _position = start;
            throw DecodeException.LengthOverflow(len, limit);
        }
        return (int)len;
    }

    private System.ReadOnlySpan<byte> TakeChecked(int len, int start)
    {
        int remaining = Remaining;
        if (len > remaining)
        {
            _position = start;
            throw DecodeException.UnexpectedEof(len, remaining);
        }
        return Take(len);
    }

    private ulong ReadLittleEndian(int byteCount)
    {
        System.ReadOnlySpan<byte> bytes = Take(byteCount);
        ulong value = 0;
        for (int i = 0; i < byteCount; i++)
        {
            value |= (ulong)bytes[i] << (8 * i);
        }
        return value;
    }

    /// Borrows the next `len` bytes and advances the cursor.
    ///
    /// The single place the remaining-bytes check lives, so no read path can
    /// allocate or index past the end.
    private System.ReadOnlySpan<byte> Take(int len)
    {
        int remaining = Remaining;
        if (len > remaining)
        {
            throw DecodeException.UnexpectedEof(len, remaining);
        }
        System.ReadOnlySpan<byte> bytes = _buffer.Slice(_position, len);
        _position += len;
        return bytes;
    }
}
"####;
