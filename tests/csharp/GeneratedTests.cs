using System;
using Xunit;

namespace Cyclonec.CSharpTests;

/// <summary>
/// The C# backend's output, compiled and run.
/// </summary>
/// <remarks>
/// The models come from <c>tests/fixtures/device_state.cs</c> and the codecs
/// from <c>tests/fixtures/cyclone.codec.cs</c>, which <c>cyclonec</c> wrote.
/// Nothing here is hand-written except the assertions - and every byte
/// expectation below is copied from <c>tests/generated.rs</c>, the Rust
/// backend's identical test, which is the whole of h.md §15's proof: the same
/// schema, read from two different syntaxes, produces the same bytes.
/// </remarks>
public sealed class GeneratedTests
{
    private static DeviceState Sample() => new DeviceState
    {
        Id = 42,
        Temperature = 21.5f,
        DisplayName = "sensor-1",
        Unrouted = 7,
        Cache = "local",
    };

    private static byte[] EncodeWith<T>(T value, Action<Writer, T> encode)
    {
        var writer = new Writer();
        encode(writer, value);
        return writer.ToArray();
    }

    // ================================================ §15 - one model, two codecs

    /// <summary>
    /// h.md §15 - <c>EdgeCodec</c> carries <c>Id</c> and <c>Temperature</c>,
    /// <c>UnityCodec</c> carries <c>Id</c> and <c>DisplayName</c>, each in
    /// declaration order. These are the same bytes the Rust backend's identical
    /// test asserts.
    /// </summary>
    [Fact]
    public void EachCodecWritesTheFieldsThatNamedIt()
    {
        Assert.Equal(
            new byte[]
            {
                0x2A, 0x00, 0x00, 0x00, // Id = 42, u32 Little Endian
                0x00, 0x00, 0xAC, 0x41, // Temperature = 21.5, raw IEEE 754 bits
            },
            EncodeWith(Sample(), DeviceStateEdgeCodec.Encode));

        Assert.Equal(
            new byte[]
            {
                0x2A, 0x00, 0x00, 0x00, // Id = 42
                0x08, 0x00, 0x00, 0x00, // "sensor-1" - a length in bytes
                0x73, 0x65, 0x6E, 0x73, 0x6F, 0x72, 0x2D, 0x31,
            },
            EncodeWith(Sample(), DeviceStateUnityCodec.Encode));
    }

    [Fact]
    public void EachCodecRoundTrips()
    {
        byte[] bytes = EncodeWith(Sample(), DeviceStateEdgeCodec.Encode);
        var value = new DeviceState();
        var reader = new Reader(bytes);

        DeviceStateEdgeCodec.Decode(ref reader, ref value);

        Assert.Equal(42u, value.Id);
        Assert.Equal(21.5f, value.Temperature);
        Assert.True(reader.IsEmpty, "the cursor lands exactly at the end");
    }

    /// <summary>
    /// A codec leaves the fields it does not carry exactly as they were, which
    /// is what lets one model be split across several of them.
    /// </summary>
    [Fact]
    public void DecodeLeavesFieldsItDoesNotCarryAlone()
    {
        byte[] bytes = EncodeWith(Sample(), DeviceStateEdgeCodec.Encode);
        var value = Sample();
        value.Id = 0;
        value.Temperature = 0f;

        var reader = new Reader(bytes);
        DeviceStateEdgeCodec.Decode(ref reader, ref value);

        Assert.Equal(42u, value.Id);
        Assert.Equal(21.5f, value.Temperature);

        Assert.Equal("sensor-1", value.DisplayName);
        Assert.Equal(7u, value.Unrouted);
        Assert.Equal("local", value.Cache);
    }

    /// <summary>Both codecs applied in turn rebuild every routed field, and only those.</summary>
    [Fact]
    public void TwoCodecsTogetherCoverTheRoutedFields()
    {
        byte[] edge = EncodeWith(Sample(), DeviceStateEdgeCodec.Encode);
        byte[] unity = EncodeWith(Sample(), DeviceStateUnityCodec.Encode);

        var value = new DeviceState();
        var edgeReader = new Reader(edge);
        DeviceStateEdgeCodec.Decode(ref edgeReader, ref value);
        var unityReader = new Reader(unity);
        DeviceStateUnityCodec.Decode(ref unityReader, ref value);

        Assert.Equal(42u, value.Id);
        Assert.Equal(21.5f, value.Temperature);
        Assert.Equal("sensor-1", value.DisplayName);

        // A field in no codec, and a field with no [Network], are on no wire.
        Assert.Equal(0u, value.Unrouted);
        Assert.Equal("", value.Cache);
    }

    // ======================================================== §16 - codec names

    /// <summary>h.md §16 - every identifier is a codec name, and the four types exist.</summary>
    [Fact]
    public void UnknownCodecNamesBecomeGeneratedTypes()
    {
        var value = new Telemetry { Sequence = 9 };
        byte[] expected = { 0x09, 0, 0, 0, 0, 0, 0, 0 };

        Assert.Equal(expected, EncodeWith(value, TelemetryEdgeCodec.Encode));
        Assert.Equal(expected, EncodeWith(value, TelemetryOrangePiCodec.Encode));
        Assert.Equal(expected, EncodeWith(value, TelemetryUnityCodec.Encode));
        Assert.Equal(expected, EncodeWith(value, TelemetryCustomACodec.Encode));
    }

    // ===================================================== §8 - composite model

    /// <summary>
    /// h.md §8 - a model-typed field becomes a call to that model's codec,
    /// inlined: no length, no delimiter, no header.
    /// </summary>
    [Fact]
    public void AModelFieldIsInlined()
    {
        var value = new Player { Hp = 100, Speed = 1.5f, Info = new PlayerInfo { Level = 3 } };

        byte[] bytes = EncodeWith(value, PlayerEdgeCodec.Encode);
        Assert.Equal(
            new byte[]
            {
                0x64, 0x00, 0x00, 0x00, // Hp = 100
                0x00, 0x00, 0xC0, 0x3F, // Speed = 1.5
                0x03, 0x00, 0x00, 0x00, // Info.Level = 3, inlined
            },
            bytes);

        var decoded = new Player { Info = new PlayerInfo() };
        var reader = new Reader(bytes);
        PlayerEdgeCodec.Decode(ref reader, ref decoded);

        Assert.Equal(100u, decoded.Hp);
        Assert.Equal(3u, decoded.Info.Level);
        Assert.True(reader.IsEmpty);
    }

    // =========================================================== §6 - Array<T>

    private static Team TeamSample() => new Team
    {
        Scores = new() { 10, 20, 30 },
        Names = new() { "alice", "bob" },
        Players = new() { new PlayerInfo { Level = 3 }, new PlayerInfo { Level = 7 } },
    };

    private static readonly byte[] TeamGoldenBytes =
    {
        0x03, 0x00, 0x00, 0x00, // Scores.Count = 3
        0x0A, 0x00, 0x00, 0x00, // Scores[0] = 10
        0x14, 0x00, 0x00, 0x00, // Scores[1] = 20
        0x1E, 0x00, 0x00, 0x00, // Scores[2] = 30
        0x02, 0x00, 0x00, 0x00, // Names.Count = 2
        0x05, 0x00, 0x00, 0x00, 0x61, 0x6C, 0x69, 0x63, 0x65, // Names[0] = "alice"
        0x03, 0x00, 0x00, 0x00, 0x62, 0x6F, 0x62, // Names[1] = "bob"
        0x02, 0x00, 0x00, 0x00, // Players.Count = 2
        0x03, 0x00, 0x00, 0x00, // Players[0].Level = 3, inlined
        0x07, 0x00, 0x00, 0x00, // Players[1].Level = 7, inlined
    };

    /// <summary>
    /// h.md §6 - <c>Array&lt;T&gt;</c> is a <c>UInt32</c> count followed by that
    /// many elements, no per-element length prefix. Same bytes as the Rust
    /// backend's identical test (<c>tests/generated.rs</c>) and the Go and
    /// GDScript backends.
    /// </summary>
    [Fact]
    public void ArrayOfScalarStringAndModelMatchesTheGoldenBytes()
    {
        Assert.Equal(TeamGoldenBytes, EncodeWith(TeamSample(), TeamEdgeCodec.Encode));
    }

    [Fact]
    public void ArrayRoundTripsIncludingNestedModelElements()
    {
        byte[] bytes = EncodeWith(TeamSample(), TeamEdgeCodec.Encode);
        var decoded = new Team();
        var reader = new Reader(bytes);
        TeamEdgeCodec.Decode(ref reader, ref decoded);

        Assert.Equal(TeamSample().Scores, decoded.Scores);
        Assert.Equal(TeamSample().Names, decoded.Names);
        Assert.Equal(3u, decoded.Players[0].Level);
        Assert.Equal(7u, decoded.Players[1].Level);
        Assert.True(reader.IsEmpty);
    }

    /// <summary>An empty <c>Array&lt;T&gt;</c> is just its <c>UInt32</c> count of zero.</summary>
    [Fact]
    public void AnEmptyArrayIsJustItsZeroCount()
    {
        byte[] bytes = EncodeWith(new Team(), TeamEdgeCodec.Encode);
        Assert.Equal(new byte[] { 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0 }, bytes);

        var decoded = new Team { Scores = new() { 1 } };
        var reader = new Reader(bytes);
        TeamEdgeCodec.Decode(ref reader, ref decoded);
        Assert.Empty(decoded.Scores);
    }

    /// <summary>
    /// <c>Limits.MaxArrayCount</c> is checked before an element is allocated,
    /// the same guard <c>MaxStringLength</c>/<c>MaxBytesLength</c> already give
    /// scalar fields.
    /// </summary>
    [Fact]
    public void AnArrayCountOverTheLimitIsRejectedBeforeAllocating()
    {
        var writer = new Writer();
        writer.WriteArrayCount(5);

        var limits = Limits.Unlimited;
        limits.MaxArrayCount = 2;
        var reader = new Reader(writer.ToArray(), limits);

        DecodeException thrown = null;
        try
        {
            reader.ReadArrayCount();
        }
        catch (DecodeException exception)
        {
            thrown = exception;
        }

        Assert.NotNull(thrown);
    }

    // ========================================================== §4 - primitives

    /// <summary>
    /// h.md §4 - each network type maps to the runtime method RFC-0002 defines,
    /// and these are the bytes that method writes.
    /// </summary>
    [Fact]
    public void EveryPrimitiveMatchesTheSpecification()
    {
        var value = new EveryPrimitive
        {
            Flag = true,
            A = -1,
            B = 255,
            C = -1,
            D = 300,
            E = -1,
            F = 0x1234_5678,
            G = -1,
            H = 1,
            I = 1.5f,
            J = 1.0,
            K = "中",
            L = new byte[] { 0xFF, 0xFE },
        };

        Assert.Equal(
            new byte[]
            {
                0x01, // bool
                0xFF, // i8 -1
                0xFF, // u8 255
                0xFF, 0xFF, // i16 -1
                0x2C, 0x01, // u16 300
                0xFF, 0xFF, 0xFF, 0xFF, // i32 -1
                0x78, 0x56, 0x34, 0x12, // u32 0x12345678 - the endianness vector
                0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, // i64 -1
                0x01, 0, 0, 0, 0, 0, 0, 0, // u64 1
                0x00, 0x00, 0xC0, 0x3F, // f32 1.5
                0, 0, 0, 0, 0, 0, 0xF0, 0x3F, // f64 1.0
                0x03, 0, 0, 0, 0xE4, 0xB8, 0xAD, // string "中" - 3 bytes, not 1 char
                0x02, 0, 0, 0, 0xFF, 0xFE, // bytes
            },
            EncodeWith(value, EveryPrimitiveAllCodec.Encode));
    }

    // ================================================ the runtime the file carries

    /// <summary>The generated file carries a conforming decoder, not a permissive one.</summary>
    [Fact]
    public void TheEmbeddedRuntimeRejectsMalformedInput()
    {
        var value = new DeviceState();

        var invalidBool = Assert.Throws<DecodeException>(() =>
        {
            var reader = new Reader(new byte[] { 0x02 });
            reader.ReadBool();
        });
        Assert.Contains("0x02", invalidBool.Message);

        var eof = Assert.Throws<DecodeException>(() =>
        {
            var reader = new Reader(new byte[] { 0x2A, 0x00, 0x00 });
            DeviceStateEdgeCodec.Decode(ref reader, ref value);
        });
        Assert.Contains("needed 4", eof.Message);

        var badUtf8 = Assert.Throws<DecodeException>(() =>
        {
            var reader = new Reader(new byte[] { 0x2A, 0, 0, 0, 0x02, 0, 0, 0, 0xFF, 0xFE });
            DeviceStateUnityCodec.Decode(ref reader, ref value);
        });
        Assert.Contains("utf-8", badUtf8.Message);
    }

    /// <summary>
    /// <c>Limits</c> reaches the generated code unchanged, so a caller can bound
    /// what an untrusted stream may allocate.
    /// </summary>
    [Fact]
    public void TheEmbeddedLimitsApplyToGeneratedDecode()
    {
        byte[] bytes = { 0x2A, 0, 0, 0, 0x10, 0, 0, 0, (byte)'0', (byte)'1', (byte)'2', (byte)'3',
            (byte)'4', (byte)'5', (byte)'6', (byte)'7', (byte)'8', (byte)'9', (byte)'a', (byte)'b',
            (byte)'c', (byte)'d', (byte)'e', (byte)'f' };

        var limits = new Limits { MaxStringLength = 8, MaxBytesLength = Limits.Unlimited.MaxBytesLength };
        var value = new DeviceState();

        var overflow = Assert.Throws<DecodeException>(() =>
        {
            var reader = new Reader(bytes, limits);
            DeviceStateUnityCodec.Decode(ref reader, ref value);
        });
        Assert.Contains("16", overflow.Message);
        Assert.Contains("8", overflow.Message);

        // The default is permissive: the same bytes decode fine.
        var permissive = new DeviceState();
        var permissiveReader = new Reader(bytes);
        DeviceStateUnityCodec.Decode(ref permissiveReader, ref permissive);
        Assert.Equal("0123456789abcdef", permissive.DisplayName);
    }

    /// <summary>
    /// Floats are written as raw bits: <c>-0.0</c> stays distinct from
    /// <c>0.0</c>, and nothing is canonicalized.
    /// </summary>
    [Fact]
    public void FloatsKeepTheirBits()
    {
        byte[] negative = EncodeWith(
            new DeviceState { Temperature = -0.0f }, DeviceStateEdgeCodec.Encode);
        byte[] positive = EncodeWith(
            new DeviceState { Temperature = 0.0f }, DeviceStateEdgeCodec.Encode);

        Assert.NotEqual(negative, positive);

        var value = new DeviceState();
        var reader = new Reader(negative);
        DeviceStateEdgeCodec.Decode(ref reader, ref value);
        Assert.True(float.IsNegative(value.Temperature));
    }

    // ============================================================== edge cases

    /// <summary>§15 - a declared codec is generated even when no field joined it.</summary>
    [Fact]
    public void ACodecNoFieldJoinedIsStillGenerated()
    {
        byte[] bytes = EncodeWith(new NoFieldsJoined { Id = 1 }, NoFieldsJoinedLonelyCodec.Encode);
        Assert.Empty(bytes);

        var value = new NoFieldsJoined();
        var reader = new Reader(Array.Empty<byte>());
        NoFieldsJoinedLonelyCodec.Decode(ref reader, ref value);
    }

    /// <summary>A type nothing marks is not a model, and its neighbours are unaffected.</summary>
    [Fact]
    public void AnUnmarkedTypeDoesNotDisturbTheNextModel()
    {
        _ = new NotAModel { Whatever = 1 };

        Assert.Equal(
            new byte[] { 0x05, 0x00, 0x00, 0x00 },
            EncodeWith(new AfterTheUnmarkedClass { Value = 5 }, AfterTheUnmarkedClassEdgeCodec.Encode));
    }

    /// <summary>Fields work exactly like properties.</summary>
    [Fact]
    public void FieldsWorkLikeProperties()
    {
        var value = new WithFields { Id = 42, Name = "Sword" };
        byte[] bytes = EncodeWith(value, WithFieldsEdgeCodec.Encode);

        Assert.Equal(
            new byte[] { 0x2A, 0, 0, 0, 0x05, 0, 0, 0, 0x53, 0x77, 0x6F, 0x72, 0x64 },
            bytes);
    }

    /// <summary>A model may be a struct.</summary>
    [Fact]
    public void StructModelsWork()
    {
        var value = new Point { X = -1, Y = 2 };
        byte[] bytes = EncodeWith(value, PointEdgeCodec.Encode);

        Assert.Equal(new byte[] { 0xFF, 0xFF, 0xFF, 0xFF, 0x02, 0, 0, 0 }, bytes);

        var decoded = new Point();
        var reader = new Reader(bytes);
        PointEdgeCodec.Decode(ref reader, ref decoded);
        Assert.Equal(-1, decoded.X);
        Assert.Equal(2, decoded.Y);
    }

    /// <summary>Encoding the same value twice produces the same bytes.</summary>
    [Fact]
    public void EncodingIsDeterministic()
    {
        Assert.Equal(
            EncodeWith(Sample(), DeviceStateEdgeCodec.Encode),
            EncodeWith(Sample(), DeviceStateEdgeCodec.Encode));
    }
}
