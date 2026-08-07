// The C# fixture the generator is tested against — the same schema as
// device_state.rs, spelled in C# attribute syntax, to prove the two produce
// the same codec names, the same field routing, and the same bytes.

using Cyclone;

/// <summary>h.md §2/§15's own example, plus the cases worth pinning.</summary>
[Network]
[Codec("edge", "unity")]
public class DeviceState
{
    [Network("u32")]
    [Codec("edge", "unity")]
    public uint Id { get; set; }

    [Network("f32")]
    [Codec("edge")]
    public float Temperature { get; set; }

    [Network("string")]
    [Codec("unity")]
    public string DisplayName { get; set; } = string.Empty;

    /// <summary>A network field in no codec: it is written by none of them.</summary>
    [Network("u32")]
    public uint Unrouted { get; set; }

    /// <summary>Not a network field at all. Logic and caches stay off the wire.</summary>
    public string Cache { get; set; } = string.Empty;
}

/// <summary>§16 — codec names the generator has never heard of.</summary>
[Network]
[Codec("edge", "orange_pi", "unity", "custom_a")]
public class Telemetry
{
    [Network("u64")]
    [Codec("edge", "orange_pi", "unity", "custom_a")]
    public ulong Sequence { get; set; }
}

/// <summary>§8 — a field whose network type is another model.</summary>
[Network]
[Codec("edge")]
public class PlayerInfo
{
    [Network("u32")]
    [Codec("edge")]
    public uint Level { get; set; }
}

[Network]
[Codec("edge")]
public class Player
{
    [Network("u32")]
    [Codec("edge")]
    public uint Hp { get; set; }

    [Network("f32")]
    [Codec("edge")]
    public float Speed { get; set; }

    [Network("PlayerInfo")]
    [Codec("edge")]
    public PlayerInfo Info { get; set; } = new PlayerInfo();
}

/// <summary>Every primitive RFC-0002 defines, once.</summary>
[Network]
[Codec("all")]
public class EveryPrimitive
{
    [Network("bool")] [Codec("all")] public bool Flag { get; set; }
    [Network("i8")] [Codec("all")] public sbyte A { get; set; }
    [Network("u8")] [Codec("all")] public byte B { get; set; }
    [Network("i16")] [Codec("all")] public short C { get; set; }
    [Network("u16")] [Codec("all")] public ushort D { get; set; }
    [Network("i32")] [Codec("all")] public int E { get; set; }
    [Network("u32")] [Codec("all")] public uint F { get; set; }
    [Network("i64")] [Codec("all")] public long G { get; set; }
    [Network("u64")] [Codec("all")] public ulong H { get; set; }
    [Network("f32")] [Codec("all")] public float I { get; set; }
    [Network("f64")] [Codec("all")] public double J { get; set; }
    [Network("string")] [Codec("all")] public string K { get; set; } = string.Empty;
    [Network("bytes")] [Codec("all")] public byte[] L { get; set; } = System.Array.Empty<byte>();
}

/// <summary>A model that declares a codec no field joined.</summary>
[Network]
[Codec("lonely")]
public class NoFieldsJoined
{
    [Network("u32")]
    public uint Id { get; set; }
}

/// <summary><c>[Network]</c> with no <c>[Codec(...)]</c>: a model, nothing to generate.</summary>
[Network]
public class NoCodecs
{
    [Network("u32")]
    [Codec("edge")]
    public uint Id { get; set; }
}

/// <summary>Nothing marks this, so it is not a model.</summary>
public class NotAModel
{
    public uint Whatever { get; set; }
}

[Network]
[Codec("edge")]
public class AfterTheUnmarkedClass
{
    [Network("u32")]
    [Codec("edge")]
    public uint Value { get; set; }
}

/// <summary>A model may be a struct.</summary>
[Network]
[Codec("edge")]
public struct Point
{
    [Network("i32")]
    [Codec("edge")]
    public int X { get; set; }

    [Network("i32")]
    [Codec("edge")]
    public int Y { get; set; }
}

/// <summary>Fields work exactly like properties.</summary>
[Network]
[Codec("edge")]
public class WithFields
{
    [Network("u32")]
    [Codec("edge")]
    public uint Id;

    [Network("string")]
    [Codec("edge")]
    public string Name = string.Empty;
}
