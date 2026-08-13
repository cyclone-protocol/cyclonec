// Player.cs is the annotated source the C# integration tests exercise - the
// C# counterpart of tests/fixtures-go/src/models/player.go: one model
// annotated in place, the generated codecs compiled against that same type,
// and the model RFC-0002 §9.1's version-skew tests decode.
namespace Models;

/// <summary>A field whose network type is another model.</summary>
[Network]
[Codec("edge")]
public class PlayerInfo
{
    [Network("u32")]
    [Codec("edge")]
    public uint Level { get; set; }
}

/// <summary>The model RFC-0002 §9.1 is tested against: three fields, and a
/// version that appends a fourth.</summary>
[Network]
[Codec("edge", "unity")]
public class Player
{
    [Network("u32")]
    [Codec("edge", "unity")]
    public uint Id { get; set; }

    [Network("f32")]
    [Codec("edge")]
    public float X { get; set; }

    [Network("f32")]
    [Codec("edge")]
    public float Y { get; set; }

    /// <summary>A network field in no codec: it is written by none of them.</summary>
    [Network("u32")]
    public uint Unrouted { get; set; }

    /// <summary>Not a network field at all. Logic and caches stay off the wire.</summary>
    public string Cache { get; set; } = "";
}

/// <summary>Holds composites: an array of primitives, an array of models, and
/// a nested model.</summary>
[Network]
[Codec("edge")]
public class Team
{
    [Network("PlayerInfo")]
    [Codec("edge")]
    public PlayerInfo Captain { get; set; } = new PlayerInfo();

    [Network("Array<string>")]
    [Codec("edge")]
    public System.Collections.Generic.List<string> Tags { get; set; } =
        new System.Collections.Generic.List<string>();

    [Network("Array<u32>")]
    [Codec("edge")]
    public System.Collections.Generic.List<uint> Scores { get; set; } =
        new System.Collections.Generic.List<uint>();

    [Network("Array<PlayerInfo>")]
    [Codec("edge")]
    public System.Collections.Generic.List<PlayerInfo> Roster { get; set; } =
        new System.Collections.Generic.List<PlayerInfo>();
}
