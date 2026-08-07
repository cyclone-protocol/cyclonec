# cyclonec

The official Cyclone **source generator**.

```
parse  →  collect  →  generate
```

`cyclonec` is not a compiler. It reads Cyclone attributes out of your sources —
Rust's `#[network]` / `#[codec(...)]`, or C#'s `[Network]` / `[Codec(...)]` —
and writes **one self-contained file per language**, then exits — the way
`protoc` does.

Self-contained means what it says: each file carries its language's Cyclone
runtime (`Writer`, `Reader`, a decode error type, `Limits`) as well as every
codec. Drop it in and it works. Nothing to import, nothing to add to
`Cargo.toml` or your `.csproj`.

It builds no schema, no IR, no type graph, no codec graph and no dependency
graph. It runs no semantic analysis and makes no second pass, and there is no
registry, no reflection and no runtime resolution. The runtime each backend
emits is a fixed block written once against RFC-0002 and copied out unchanged —
nothing about the wire format is worked out per model, per field, per run, or
per language.

Zero dependencies, in the generator and in what it generates.

## Two syntaxes, one schema

```rust
#[network]
#[codec(edge, unity)]
struct DeviceState {
    #[network(u32)]
    #[codec(edge, unity)]
    id: u32,
}
```

```csharp
[Network]
[Codec("edge", "unity")]
public class DeviceState
{
    [Network("u32")]
    [Codec("edge", "unity")]
    public uint Id { get; set; }
}
```

Both produce `DeviceStateEdgeCodec` and `DeviceStateUnityCodec`, both route
`Id`/`id` into each, and both write the identical four Little Endian bytes for
it. Rust and C# are read by two independent scanners into one shared shape
(`Model` / `Field` — see [`src/model.rs`](src/model.rs)), so a schema means the
same thing whichever syntax it was written in: same codec names, same field
routing, same bytes. What differs between the two outputs is only the syntax
and the runtime method names — never the wire format, per h.md §2.

## Usage

```
cyclonec --out <PATH> [OPTIONS] <PATH>...
```

| Option | Effect |
|--------|--------|
| `-o, --out <PATH>` | **required** — where the generated file(s) go |
| `--check` | report an out-of-date output file, write nothing, exit 1 if stale |
| `--stdout` | print instead of writing; replaces `--out` rather than joining it |
| `-q, --quiet` | report only errors |
| `-h, --help` / `-V, --version` | |

A directory is searched recursively for `.rs` **and** `.cs` files, skipping
`target` and the generator's own output. Each file's extension picks its
scanner; nothing else about it is inspected to decide.

`--out` decides the destination by its own extension, not by what already
exists on disk, so a first run and a second run agree:

| `--out` | Rust goes to | C# goes to |
|---------|---------------|------------|
| `src/` | `src/cyclone.codec.rs` | `src/cyclone.codec.cs` |
| `src/net.rs` | `src/net.rs`, exactly | `src/net.cs` (only if C# models exist) |
| `src/net.cs` | `src/net.rs` (only if Rust models exist) | `src/net.cs`, exactly |

Either output is written only if that language actually has a model that
declared a codec — a Rust-only project never sees a `.cs` file appear, and vice
versa.

```
cyclonec --out src/ src/                 # a whole tree, split by language
cyclonec --out src/net.rs src/models.rs  # one named Rust file
cyclonec --out src/net.cs src/Models.cs  # one named C# file
cyclonec --check --out src/ src/         # CI: fail if either is out of date
```

`--out` is required on purpose: the output is one file per language holding a
whole project's codecs, and guessing where that belongs is not the generator's
call. There is deliberately **no `--codec` flag** either — a model declares its
codecs in the source, and asking again on the command line could only ever
disagree.

## What it reads

Four markers per language, matched in meaning:

| | Rust | C# |
|-|------|-----|
| this type is a model | `#[network]` | `[Network]` |
| generate these codecs | `#[codec(edge, unity)]` | `[Codec("edge", "unity")]` |
| this field's wire type | `#[network(u32)]` | `[Network("u32")]` |
| this field's codecs | `#[codec(edge)]` | `[Codec("edge")]` |

The C# attributes come from [`cyclone-attributes`](../cyclone-attributes-csharp)
(namespace `Cyclone`); `cyclonec` reads the text of `[Network("...")]` and
`[Codec(...)]` directly out of the source; it does not reference that package,
load it, or run any C# at all.

```rust
#[network]
#[codec(edge, unity)]        // generate these codecs for it
struct DeviceState {
    #[network(u32)]          // this field's network type
    #[codec(edge, unity)]    // this field is in both codecs
    id: u32,

    #[network(f32)]
    #[codec(edge)]           // edge only
    temperature: f32,

    #[network(string)]
    #[codec(unity)]          // unity only
    display_name: String,
}
```

generates exactly two codecs — `DeviceStateEdgeCodec` (`id`, `temperature`) and
`DeviceStateUnityCodec` (`id`, `display_name`) — never a third, never one fewer.
The C# spelling of the same model generates the identical two, with `Id`,
`Temperature`, `DisplayName` in place of the lowercase field names.

**The wire type is never inferred from the host type**, in either language.
`[Network("u32")] public ulong Value` is a `u32` on the wire — four bytes,
Little Endian — exactly as `#[network(u32)] value: u64` is in Rust. Whether the
host language accepts the resulting call (`WriteUInt32(value.Value)` with
`Value` a `ulong`) is that language's compiler's question, not this generator's
— see h.md §2 and §13, and `tests/cli.rs`'s
`csharp_native_type_does_not_change_the_wire_type` for the check.

## What it writes

```rust
/// The `edge` codec for [`DeviceState`], generated from its Cyclone attributes.
pub struct DeviceStateEdgeCodec;

impl DeviceStateEdgeCodec {
    pub fn encode(writer: &mut Writer, value: &DeviceState) {
        writer.write_u32(value.id);
        writer.write_f32(value.temperature);
    }

    pub fn decode(reader: &mut Reader, value: &mut DeviceState) -> Result<(), DecodeError> {
        value.id = reader.read_u32()?;
        value.temperature = reader.read_f32()?;
        Ok(())
    }
}
```

```csharp
/// <summary>The <c>edge</c> codec for <see cref="DeviceState"/>, generated from its Cyclone attributes.</summary>
public static class DeviceStateEdgeCodec
{
    public static void Encode(Writer writer, DeviceState value)
    {
        writer.WriteUInt32(value.Id);
        writer.WriteFloat32(value.Temperature);
    }

    public static void Decode(ref Reader reader, ref DeviceState value)
    {
        value.Id = reader.ReadUInt32();
        value.Temperature = reader.ReadFloat32();
    }
}
```

`decode`/`Decode` fills only the fields its codec carries, leaving the rest as
they were — what lets one model be split across several codecs, on both sides.
(C#'s `Reader` is a `ref struct`, so both it and the model are threaded through
by `ref`; a nested model field, which C# cannot pass a property by `ref` into
directly, is decoded through a local instead — read it out, decode into it,
assign it back. See the header of
[`src/generator/csharp.rs`](src/generator/csharp.rs).)

Above the codecs, the same file carries the runtime they call — `Writer`,
`Reader`, a decode error type, `Limits`, one method per Cyclone primitive. That
block is identical in every file of its language `cyclonec` writes. It is not
generated from your models — it is a constant per language, written once
against RFC-0002 ([`rust_runtime.rs`](src/generator/rust_runtime.rs),
[`csharp_runtime.rs`](src/generator/csharp_runtime.rs)). §10 forbids the
generator from working out byte layout, and it does not: it only knows how to
copy these out.

Rust names its models unqualified and needs an `include!`:

```rust
include!("cyclone.codec.rs");
```

C# needs nothing at all — add the file to your project (or let the SDK's
default `**/*.cs` glob find it) and it compiles, provided your models are
visible from wherever the file ends up (same file, same namespace, or no
namespace — `cyclonec` does not track C# namespaces, the same simplification
Rust's scanner already makes for `mod`).

### Three rules, and no fourth, on both sides

| Network type | Rust | C# |
|--------------|------|-----|
| a primitive | `writer.write_u32(value.id)` / `reader.read_u32()?` | `writer.WriteUInt32(value.Id)` / `reader.ReadUInt32()` |
| a model | `PlayerInfoEdgeCodec::encode(writer, &value.info)` | `PlayerInfoEdgeCodec.Encode(writer, value.Info)` |
| a field in no codec | nothing, by any codec | nothing, by any codec |

Rust passes `string`/`bytes` by reference; C# needs no such distinction —
`string` and `byte[]` are already reference types.

**No byte layout is derived**, in either backend. Endianness, length prefixes
and string encoding live in the carried runtime block, not in anything the
generator computes. Deriving them per model — or per language — is how two
implementations of one wire format start disagreeing, and h.md §15 makes
avoiding that a hard requirement: the same schema must produce the same bytes
in Rust, C#, or any future backend.

**Nothing is generated that is reached at runtime.** No `encoded_size`, no
`CodecRegistry`, no `get_codec`/`GetCodec`, no type id, no `Box<dyn …>`, no
reflection. A codec is a name known at compile time, and the caller names it
directly.

## What it validates

One thing, per language:

```
error: player.rs:5: #[network] field requires a network type
error: Player.cs:5: [Network] field requires a wire type: [Network("...")]
```

Everything else is the host compiler's. A field declared with a mismatched
native type is generated as its declared wire type without comment; a call to
a codec that does not exist is spelled and left for `rustc` or the C# compiler
to name. `cyclonec` does not become a second compiler for either language.

## Reading, not parsing

Neither scanner is a parser for its language. Neither knows types, traits,
generics, namespaces, or any other semantic of Rust or C# — the host compiler
already does, and running a second copy of it to find four markers would be the
slowest possible way to answer the smallest possible question.

The one thing each must get right is *where a token is*: a `[Network]` (or
`#[network]`) inside a string, or `class`/`struct` inside a comment, must not
be mistaken for source. That is the only reason each is a lexer and not a
substring search, and both are tested as such.

## Layout

```
cyclonec/
├── src/
│   ├── main.rs             collect files, resolve --out, write or check
│   ├── cli.rs               seven flags
│   ├── model.rs              Model / Field — the shape both scanners produce
│   ├── parser/
│   │   ├── mod.rs            picks a scanner by extension
│   │   ├── rust.rs           lexer + scanner for #[network] / #[codec(...)]
│   │   └── csharp.rs         lexer + scanner for [Network] / [Codec(...)]
│   └── generator/
│       ├── mod.rs
│       ├── rust.rs           Model → Rust source
│       ├── rust_runtime.rs   the RFC-0002 block, as one Rust constant
│       ├── csharp.rs         Model → C# source
│       └── csharp_runtime.rs the RFC-0002 block, as one C# constant
├── tests/
│   ├── cli.rs                drives the real binary, both languages
│   ├── generated.rs          compiles + runs the committed Rust output
│   ├── fixtures/             the schema, once per language
│   └── csharp/               a dotnet project: compiles + runs the C# output
└── README.md
```

Adding a third language is the same shape again: a `parser/<lang>.rs` reading
that language's attribute syntax into the same `Model` / `Field`, and a
`generator/<lang>.rs` + `generator/<lang>_runtime.rs` pair writing it back out.
Neither of the two existing backends imports the other, and neither would need
to change.

## Tests

`tests/generated.rs` includes the committed `tests/fixtures/cyclone.codec.rs`
into a real crate and runs it, so everything it asserts is code `rustc`
accepted. Because the file carries its own runtime, there is no stub and no
import: the assertions are **real wire bytes**, compared against RFC-0002 —
including the endianness vector, `-0.0` keeping its sign, a `string` length
counted in bytes, and the decoder rejecting an invalid `bool`, a truncated
read, bad UTF-8, and a length past its `Limits`.

`tests/csharp/` is the same proof for the C# backend, as a real `dotnet test`
project: it compiles `tests/fixtures/cyclone.codec.cs` against
`tests/fixtures/device_state.cs` and asserts the **identical byte sequences**
`tests/generated.rs` asserts for the Rust side — which is h.md §15's
cross-language requirement, checked rather than assumed.

`tests/cli.rs` drives the real binary over real files, for both languages: the
codecs a model declares, PascalCase names, the one error per language, where
`--out` writes for each extension, a directory holding both languages at once,
aggregation of several sources into one file, `--check`, `--stdout`, each
lexer refusing to see models in comments and strings, and the native-type
independence case (`[Network("u32")] public ulong Value` reports wire type
`u32`, checked against the generator's own output rather than by compiling it —
h.md §2 leaves the compiling part to the C# compiler).

```
cargo test                        # the generator, both frontends and backends
cd tests/csharp && dotnet test    # the C# backend's output, compiled and run
```

## References

- RFC-0001 — What Cyclone is
- RFC-0002 — Binary Reader / Writer API, which names every method generated here
- RFC-0003 — Conformance

## License

Apache-2.0
