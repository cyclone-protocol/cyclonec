# cyclonec

The official Cyclone **source generator**.

```
parse  →  collect  →  generate
```

`cyclonec` is not a compiler. It reads Cyclone attributes out of your sources -
Rust's `#[network]` / `#[codec(...)]`, C#'s `[Network]` / `[Codec(...)]`, or
Go's `//cyclone:model` directive and `cyclone:"..."` / `codec:"..."` struct
tags - and writes **one self-contained file per language**, then exits - the
way `protoc` does.

Self-contained means what it says: each file carries its language's Cyclone
runtime (`Writer`, `Reader`, a decode error type, `Limits`) as well as every
codec. Drop it in and it works. Nothing to import, nothing to add to
`Cargo.toml`, your `.csproj`, or `go.mod`.

It builds no schema, no IR, no type graph, no codec graph and no dependency
graph. It runs no semantic analysis and makes no second pass, and there is no
registry, no reflection and no runtime resolution. The runtime each backend
emits is a fixed block written once against RFC-0002 and copied out unchanged -
nothing about the wire format is worked out per model, per field, per run, or
per language.

Zero dependencies, in the generator and in what it generates. (The Go backend
uses only `encoding/binary`, `math`, `fmt` and `unicode/utf8` - all standard
library.)

## Three syntaxes, one schema

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

```go
//cyclone:model codec=edge,unity
type DeviceState struct {
    ID uint32 `cyclone:"u32" codec:"edge,unity"`
}
```

All three produce `DeviceStateEdgeCodec` and `DeviceStateUnityCodec`, all three
route `id`/`Id`/`ID` into each, and all three write the identical four Little
Endian bytes for it. The three source languages are read by independent
scanners into one shared shape (`Model` / `Field` - see
[`src/model.rs`](src/model.rs)), so a schema means the same thing whichever
syntax it was written in: same codec names, same field routing, same bytes.
What differs between the three outputs is only the syntax and the runtime
method names - never the wire format.

## Usage

```
cyclonec --out <PATH> [OPTIONS] <PATH>...
```

| Option | Effect |
|--------|--------|
| `-o, --out <PATH>` | **required** - where the generated file(s) go |
| `--check` | report an out-of-date output file, write nothing, exit 1 if stale |
| `--stdout` | print instead of writing; replaces `--out` rather than joining it |
| `-q, --quiet` | report only errors |
| `-h, --help` / `-V, --version` | |

A directory is searched recursively for `.rs`, `.cs` **and** `.go` files,
skipping `target` and the generator's own output. Each file's extension picks
its scanner; nothing else about it is inspected to decide.

`--out` decides the destination by its own extension, not by what already
exists on disk, so a first run and a second run agree:

| `--out` | Rust goes to | C# goes to | Go goes to |
|---------|---------------|------------|------------|
| `src/` | `src/cyclone.codec.rs` | `src/cyclone.codec.cs` | `src/cyclone.codec.go` |
| `src/net.rs` | `src/net.rs`, exactly | `src/net.cs` (if C# models exist) | `src/net.go` (if Go models exist) |
| `src/net.go` | `src/net.rs` (if Rust models exist) | `src/net.cs` (if C# models exist) | `src/net.go`, exactly |

Any of the three is written only if that language actually has a model that
declared a codec - a Rust-only project never sees a `.cs` or `.go` file appear.

```
cyclonec --out src/ src/                 # a whole tree, split by language
cyclonec --out src/net.rs src/models.rs  # one named Rust file
cyclonec --out src/net.go src/models.go  # one named Go file
cyclonec --check --out src/ src/         # CI: fail if any is out of date
```

`--out` is required on purpose: the output is one file per language holding a
whole project's codecs, and guessing where that belongs is not the generator's
call. There is deliberately **no `--codec` flag** either - a model declares its
codecs in the source, and asking again on the command line could only ever
disagree.

## What it reads

Four markers per language, matched in meaning:

| | Rust | C# | Go |
|-|------|-----|-----|
| this type is a model | `#[network]` | `[Network]` | `//cyclone:model` |
| generate these codecs | `#[codec(edge, unity)]` | `[Codec("edge", "unity")]` | `//cyclone:model codec=edge,unity` |
| this field's wire type | `#[network(u32)]` | `[Network("u32")]` | `` `cyclone:"u32"` `` |
| this field's codecs | `#[codec(edge)]` | `[Codec("edge")]` | `` `codec:"edge"` `` |

The C# attributes come from [`cyclone-attributes`](../cyclone-attributes-csharp)
(namespace `Cyclone`). Go has no attribute mechanism, so its model marker and
its codec list share one comment directive:

```go
//cyclone:model codec=edge,unity
type DeviceState struct {
    ID          uint32  `cyclone:"u32" codec:"edge,unity"`
    Temperature float32 `cyclone:"f32" codec:"edge"`
    Name        string  `cyclone:"string" codec:"unity"`
}
```

generates exactly two codecs - `DeviceStateEdgeCodec` (`ID`, `Temperature`) and
`DeviceStateUnityCodec` (`ID`, `Name`) - never a third, never one fewer. The
Rust and C# spellings of the same model generate the identical two.

**The wire type is never inferred from the host type**, in any of the three.
`` `cyclone:"u32"` `` on a `uint64` field is a `u32` on the wire - four bytes,
Little Endian - exactly as `#[network(u32)] value: u64` is in Rust and
`[Network("u32")] public ulong Value` is in C#. Whether the host language
accepts the resulting call (`w.WriteU32(value.ID)` with `ID` a `uint64`) is
that language's compiler's question, not this generator's.

### The one thing Go requires that the others don't

A `//cyclone:model` directive is a comment - nothing in the language ties it to
the declaration after it the way an attribute is tied to what it decorates. So
`cyclonec` checks: the very next thing after the directive must be
`type Name struct { ... }`, or it is a reported error, never a silent skip
(h.md §12). A directive on a `type Count int`, or one followed by a `func`, or
one at the end of a file, all fail loudly rather than vanishing.

Similarly, a field tagged `` `codec:"edge"` `` with no `` `cyclone:"..."` ``
tag at all is an error - the Go counterpart of Rust's "field requires a
network type" and C#'s "field requires a wire type" - rather than being
silently dropped, since a field that named a codec was clearly meant to be on
the wire.

## What it writes

```rust
/// The `edge` codec for [`DeviceState`], generated from its Cyclone attributes.
pub struct DeviceStateEdgeCodec;

impl DeviceStateEdgeCodec {
    pub fn encode(writer: &mut Writer, value: &DeviceState) {
        writer.write_u32(value.id);
    }

    pub fn decode(reader: &mut Reader, value: &mut DeviceState) -> Result<(), DecodeError> {
        value.id = reader.read_u32()?;
        Ok(())
    }
}
```

```csharp
public static class DeviceStateEdgeCodec
{
    public static void Encode(Writer writer, DeviceState value)
    {
        writer.WriteUInt32(value.Id);
    }

    public static void Decode(ref Reader reader, ref DeviceState value)
    {
        value.Id = reader.ReadUInt32();
    }
}
```

```go
type DeviceStateEdgeCodec struct{}

func (DeviceStateEdgeCodec) Encode(w *Writer, value *DeviceState) {
	w.WriteU32(value.ID)
}

func (DeviceStateEdgeCodec) Decode(r *Reader, value *DeviceState) error {
	var err error
	value.ID, err = r.ReadU32()
	if err != nil {
		return err
	}
	return nil
}
```

`decode`/`Decode` fills only the fields its codec carries, leaving the rest as
they were - what lets one model be split across several codecs, on all three
sides. Go has no exceptions, so it is the one language where this is spelled
out explicitly: every read is followed by its own `if err != nil { return err
}`, the shape h.md §10 itself specifies.

(C#'s `Reader` is a `ref struct`, so both it and the model are threaded through
by `ref`; a nested model field, which C# cannot pass a property by `ref` into
directly, is decoded through a local instead. Go needs no such workaround - a
struct field reached through a pointer is directly addressable, so
`&value.Info` works exactly like Rust's `&mut value.info`. See the header of
[`src/generator/csharp.rs`](src/generator/csharp.rs) for the C# case in full.)

Above the codecs, the same file carries the runtime they call - `Writer`,
`Reader`, a decode error type, `Limits`, one method per Cyclone primitive. That
block is identical in every file of its language `cyclonec` writes. It is not
generated from your models - it is a constant per language, written once
against RFC-0002 ([`rust_runtime.rs`](src/generator/rust_runtime.rs),
[`csharp_runtime.rs`](src/generator/csharp_runtime.rs),
[`go_runtime.rs`](src/generator/go_runtime.rs)). §10/§6 forbid the generator
from working out byte layout, and it does not: it only knows how to copy these
out.

Rust names its models unqualified and needs an `include!`. C# needs nothing at
all - add the file to your project and it compiles. **Go needs its `package`
line to match**: Go compiles by directory, not by file, so the generated file
declares whichever `package` the first Go source `cyclonec` read declared, and
belongs beside it in the same directory.

### Three rules, and no fourth, on every backend

| Network type | Rust | C# | Go |
|--------------|------|-----|-----|
| a primitive | `writer.write_u32(value.id)` | `writer.WriteUInt32(value.Id)` | `w.WriteU32(value.ID)` |
| a model | `PlayerInfoEdgeCodec::encode(writer, &value.info)` | `PlayerInfoEdgeCodec.Encode(writer, value.Info)` | `(PlayerInfoEdgeCodec{}).Encode(w, &value.Info)` |
| a field in no codec | nothing, by any codec | nothing, by any codec | nothing, by any codec |

**No byte layout is derived**, in any backend. Endianness, length prefixes and
string encoding live in the carried runtime block, not in anything the
generator computes. Deriving them per model - or per language - is how two
implementations of one wire format start disagreeing, and this is a hard
requirement: the same schema must produce the same bytes in Rust, C#, Go, or
any future backend.

**Nothing is generated that is reached at runtime.** No `encoded_size`, no
`CodecRegistry`, no `GetCodec`, no type id, no `interface{}` dispatch, no
reflection. A codec is a name known at compile time, and the caller names it
directly.

## What it validates

Per-language parse errors - one that every backend shares, and one that is
Go's alone:

```
error: player.rs:5: #[network] field requires a network type
error: Player.cs:5: [Network] field requires a wire type: [Network("...")]
error: player.go:5: field 'ID' is missing cyclone wire type
error: player.go:3: //cyclone:model must be immediately followed by a `type Name struct { ... }` declaration
```

And one check that runs across a whole language's models after parsing, before
anything is rendered: a field naming a nested model carries its own codec
membership into the nested call (`Player.info` routed into `edge` makes
`PlayerEdgeCodec` call `PlayerInfoEdgeCodec`), and that call only makes sense if
the referenced model actually declares that codec. If it does not -
`PlayerInfo` never declared `orange_pi`, but `Player.info` routed into it
anyway - `cyclonec` reports it immediately:

```
error: model 'Player' field 'info' routes into codec 'orange_pi', but the model
it references, 'PlayerInfo', declares only: edge, unity - 'PlayerInfoOrangePiCodec'
would never be generated
```

This is the one validation that spans more than one model, and it is the same
check for all three languages - [`model::check_nested_codecs`](src/model.rs)
reads only the shared `Model` / `Field` shape, so it needed writing once. It
only fires for a model *this run parsed*; a field naming a type from elsewhere
(hand-written, another package) is unaffected - that resolution is still left
to the host compiler, same as ever.

Everything else is the host compiler's. A field declared with a mismatched
native type is generated as its declared wire type without comment; a call to
a codec this run never heard of at all is spelled and left for `rustc`, the C#
compiler, or `go build` to name. `cyclonec` does not become a second compiler
for any of the three.

## Reading, not parsing

None of the three scanners is a parser for its language. None knows types,
traits, generics, namespaces, or any other semantic of Rust, C# or Go - the
host compiler already does, and running a second copy of it to find a handful
of markers would be the slowest possible way to answer the smallest possible
question. (h.md §11 suggests Go's own `go/parser` and `go/ast`; a fourth
dependency to reach the same four facts the other two scanners already reach
by hand would be inconsistent with that stance and with "chỉ đọc, không
compile" - so Go gets the same hand-rolled lexer treatment Rust and C# already
have.)

The one thing each must get right is *where a token is*: a `[Network]` (or
`#[network]`, or `//cyclone:model`) inside a string, or `class`/`struct`
inside a comment, must not be mistaken for source. That is the only reason
each is a lexer and not a substring search, and all three are tested as such.

Go's lexer carries one more responsibility the other two don't: it must tell
its one *significant* comment (`//cyclone:model ...`) apart from the countless
ordinary ones sitting right next to it in the same syntax, rather than
discarding every comment as noise the way Rust's and C#'s lexers do.

## Layout

```
cyclonec/
├── src/
│   ├── main.rs             collect files, resolve --out, write or check
│   ├── cli.rs               seven flags
│   ├── model.rs              Model / Field - the shape all three scanners produce
│   ├── parser/
│   │   ├── mod.rs            picks a scanner by extension
│   │   ├── rust.rs           lexer + scanner for #[network] / #[codec(...)]
│   │   ├── csharp.rs         lexer + scanner for [Network] / [Codec(...)]
│   │   └── go.rs             lexer + scanner for //cyclone:model + struct tags
│   └── generator/
│       ├── mod.rs
│       ├── rust.rs           Model → Rust source
│       ├── rust_runtime.rs   the RFC-0002 block, as one Rust constant
│       ├── csharp.rs         Model → C# source
│       ├── csharp_runtime.rs the RFC-0002 block, as one C# constant
│       ├── go.rs             Model → Go source
│       └── go_runtime.rs     the RFC-0002 block, as one Go constant
├── tests/
│   ├── cli.rs                drives the real binary, all three languages
│   ├── generated.rs          compiles + runs the committed Rust output
│   ├── fixtures/             the schema, once per language, plus a go.mod
│   └── csharp/               a dotnet project: compiles + runs the C# output
└── README.md
```

A fourth language is the same shape again: a `parser/<lang>.rs` reading that
language's own way of spelling the four markers into the same `Model` /
`Field`, and a `generator/<lang>.rs` + `generator/<lang>_runtime.rs` pair
writing it back out. None of the three existing backends imports another, and
none would need to change.

## Tests

`tests/generated.rs` includes the committed `tests/fixtures/cyclone.codec.rs`
into a real crate and runs it, so everything it asserts is code `rustc`
accepted. Because the file carries its own runtime, there is no stub and no
import: the assertions are **real wire bytes**, compared against RFC-0002 -
including the endianness vector, `-0.0` keeping its sign, a `string` length
counted in bytes, and the decoder rejecting an invalid `bool`, a truncated
read, bad UTF-8, and a length past its `Limits`.

`tests/csharp/` is the same proof for the C# backend, as a real `dotnet test`
project. `tests/fixtures/` is the same proof again for Go, as a real `go test`
package: `device_state.go` (the models), `cyclone.codec.go` (what `cyclonec`
generated from them) and `cyclone_generated_test.go` (hand-written assertions)
all live in one Go package, the way Go tests conventionally do, and every byte
expectation is copied line for line from the Rust and C# versions of the same
test. All three assert the **identical byte sequences** for the same schema -
the cross-language requirement this project exists to hold, checked rather
than assumed.

`tests/cli.rs` drives the real binary over real files, for all three
languages: the codecs a model declares, PascalCase names, each language's
error(s), where `--out` writes for each extension, a directory holding all
three languages at once, aggregation of several sources into one file per
language, `--check`, `--stdout`, each lexer refusing to see models in comments
and strings, the native-type independence case per language (checked against
the generator's own output text rather than by compiling it), and - for
Go specifically - the directive-must-precede-a-struct rule, the
directive-word-boundary rule, and the `package` clause being carried into the
generated file.

```
cargo test                            # the generator, all three frontends and backends
cd tests/csharp && dotnet test        # the C# backend's output, compiled and run
cd tests/fixtures && go test ./...    # the Go backend's output, compiled and run
```

## References

- RFC-0001 - What Cyclone is
- RFC-0002 - Binary Reader / Writer API, which names every method generated here
- RFC-0003 - Conformance

## License

Apache-2.0
