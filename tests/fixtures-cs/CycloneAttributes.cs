// The `Network`/`Codec` attribute pair the README promises: "a few lines
// your models depend on, not `cyclonec` itself" - the C# counterpart of the
// `cyclone-attributes` crate for Rust and `cyclone.h` for C++/C. `cyclonec`
// reads these two names off `src/models/Player.cs` as plain text; neither
// attribute carries any behavior of its own, so a model compiles unchanged
// whether or not `cyclonec` ever runs over it.
//
// Deliberately at the project root, not under `src/`: `cyclonec generate`
// walks `src/` looking for models, and this file declares none - keeping it
// out of that tree is the same choice `tests/fixtures-cpp/include/cyclone.h`
// and `tests/fixtures-c/include/cyclone.h` make for the same reason.
//
// No namespace: `src/models/Player.cs` uses `[Network]`/`[Codec(...)]` bare,
// with no `using` directive, so these have to be reachable from the global
// namespace.

using System;

[AttributeUsage(
    AttributeTargets.Class | AttributeTargets.Struct | AttributeTargets.Property
        | AttributeTargets.Field,
    AllowMultiple = false)]
public sealed class NetworkAttribute : Attribute
{
    /// The field's wire type (e.g. "u32", "string", "Array<u32>") - absent
    /// on the type-level `[Network]` marker, which takes no argument at all.
    public string WireType { get; }

    public NetworkAttribute()
    {
    }

    public NetworkAttribute(string wireType)
    {
        WireType = wireType;
    }
}

[AttributeUsage(
    AttributeTargets.Class | AttributeTargets.Struct | AttributeTargets.Property
        | AttributeTargets.Field,
    AllowMultiple = false)]
public sealed class CodecAttribute : Attribute
{
    /// The codec names: every one this model generates, above the type; the
    /// ones a field belongs to, above the field.
    public string[] Names { get; }

    public CodecAttribute(params string[] names)
    {
        Names = names;
    }
}
