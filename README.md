# cyclonec

The official Cyclone **source generator**.

```
parse  →  collect  →  generate
```

`cyclonec` is not a compiler. It reads Cyclone attributes out of your sources,
writes **one self-contained file**, and exits - the way `protoc` does.

Self-contained means what it says: the file carries the Cyclone runtime
(`Writer`, `Reader`, `DecodeError`, `Limits`) as well as every codec. Include it
and it works. Nothing to import, nothing to add to `Cargo.toml`.

It builds no schema, no IR, no type graph, no codec graph and no dependency
graph. It runs no semantic analysis and makes no second pass, and there is no
registry, no reflection and no runtime resolution. The runtime it emits is a
fixed block written once against RFC-0002 and copied out unchanged - nothing
about the wire format is worked out per model, per field, or per run.

Zero dependencies, in the generator and in what it generates.

## Usage

```
cyclonec --out <PATH> [OPTIONS] <PATH>...
```

| Option | Effect |
|--------|--------|
| `-o, --out <PATH>` | **required** - where the generated file goes |
| `--check` | report an out-of-date output file, write nothing, exit 1 if stale |
| `--stdout` | print instead of writing; replaces `--out` rather than joining it |
| `-q, --quiet` | report only errors |
| `-h, --help` / `-V, --version` | |

`--out` decides the destination by its extension, not by what already exists on
disk, so a first run and a second run agree:

| `--out` | Writes |
|---------|--------|
| `src/` | `src/cyclone.codec.rs` |
| `src/gen` | `src/gen/cyclone.codec.rs` (creating it) |
| `src/net.rs` | `src/net.rs`, exactly |

```
cyclonec --out src/ src/                 # a whole tree into src/cyclone.codec.rs
cyclonec --out src/net.rs src/models.rs  # one named file
cyclonec --check --out src/ src/         # CI: fail if it is out of date
```

Every source lands in **one** output file, not one file each. A directory is
searched recursively for `.rs` files, skipping `target` and the generator's own
`*.codec.rs`.

`--out` is required on purpose: the output is a single file holding a whole
project's codecs, and guessing where that belongs is not the generator's call.

There is deliberately **no `--codec` flag** either. A model declares its codecs
in the source, and asking for them again on the command line could only ever
disagree.

## What it reads

Four markers, from [`cyclone-attributes`](../cyclone-attributes):

```rust
#[network]                   // this struct is a network model
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

and generates exactly two codecs:

```
DeviceStateEdgeCodec    →  id, temperature
DeviceStateUnityCodec   →  id, display_name
```

Never a third, never one fewer.

## What it writes

```rust
/// The `edge` codec for [`DeviceState`], generated from its Cyclone attributes.
pub struct DeviceStateEdgeCodec;

impl DeviceStateEdgeCodec {
    /// Writes the `edge` fields of `value`, in declaration order.
    pub fn encode(writer: &mut Writer, value: &DeviceState) {
        writer.write_u32(value.id);
        writer.write_f32(value.temperature);
    }

    /// Reads the `edge` fields into `value`, in declaration order.
    pub fn decode(reader: &mut Reader, value: &mut DeviceState) -> Result<(), DecodeError> {
        value.id = reader.read_u32()?;
        value.temperature = reader.read_f32()?;
        Ok(())
    }
}
```

`decode` takes `&mut value` and fills only the fields its codec carries, leaving
the rest as they were. That is what lets one model be split across several
codecs.

Above the codecs, the same file carries the runtime they call:

```rust
pub enum DecodeError { UnexpectedEof { .. }, InvalidBool(u8), InvalidUtf8, LengthOverflow { .. } }
pub struct Limits { pub max_string_len: usize, pub max_bytes_len: usize }
pub struct Writer { /* write_bool, write_u32, write_string, … */ }
pub struct Reader<'a> { /* read_bool, read_u32, read_string, … */ }
```

That block is identical in every file `cyclonec` writes. It is not generated
from your models - it is a constant, written once against RFC-0002. §10 forbids
the generator from working out byte layout, and it does not: it only knows how
to copy this out.

The models are named unqualified, so include the file where they are in scope:

```rust
include!("cyclone.codec.rs");
```

### Three rules, and no fourth

| Network type | Emitted |
|--------------|---------|
| `bool` `i8` `u8` `i16` `u16` `i32` `u32` `i64` `u64` `f32` `f64` `string` `bytes` | `writer.write_u32(…)` / `reader.read_u32()?` |
| anything else | another model: `PlayerInfoEdgeCodec::encode(…)` |
| a field in no `#[codec(...)]` | nothing, by any codec |

`string` and `bytes` are passed by reference; every scalar by value.

**No byte layout is derived.** Endianness, length prefixes and string encoding
live in the carried runtime block, not in anything the generator computes.
Deriving them per model is how two implementations of one wire format start
disagreeing.

**Nothing is generated that is reached at runtime.** No `encoded_size`, no
`CodecRegistry`, no `get_codec`, no type id, no `Box<dyn …>`. A codec is a name
known at compile time, and the caller names it directly.

## What it validates

One thing:

```
error: player.rs:5: #[network] field requires a network type
```

Everything else is `rustc`'s. A field declared `u64` and annotated
`#[network(u32)]` is generated as a `u32` without comment; a call to a codec that
does not exist is spelled and left for the compiler to name. `cyclonec` does not
become a second Rust compiler.

## Reading, not parsing

The parser is not a Rust parser and never will be. It knows no types, traits,
lifetimes, modules or borrowing - `rustc` already does, and running a second copy
of it to find four markers would be the slowest possible way to answer the
smallest possible question.

The one thing it must get right is *where a token is*: a `#[network]` inside a
string and a `struct` inside a comment are not source. That is the only reason it
is a lexer and not a substring search, and it is tested as such.

## Layout

```
cyclonec/
├── src/
│   ├── main.rs        collect files, resolve --out, write or check
│   ├── cli.rs         six flags
│   ├── parser.rs      lexer + scanner - the only file that reads source
│   ├── model.rs       what the parser collected; not an IR
│   ├── generator.rs   models → source, one field at a time
│   └── runtime.rs     the RFC-0002 block, as one constant
├── tests/
└── README.md
```

### Other languages

The seam for C# or Go is the primitive table and the `push_str` calls in
`generator.rs` - a second table and a second set of writes, in that file. That is
not worth a trait, a registry of backends, or a directory of modules until there
is a second one to compare against. Rust is the backend that exists.

## Tests

`tests/generated.rs` includes the committed `tests/fixtures/cyclone.codec.rs`
into a real crate and runs it, so everything it asserts is code `rustc` accepted.
Because the file carries its own runtime, there is no stub and no import: the
assertions are **real wire bytes**, compared against RFC-0002 - including the
endianness vector, `-0.0` keeping its sign, a `string` length counted in bytes,
and the decoder rejecting an invalid `bool`, a truncated read, bad UTF-8 and a
length past its `Limits`.

`tests/cli.rs` drives the real binary over real files: the codecs a model
declares, PascalCase names, the one error, where `--out` writes, aggregation of
several sources into one file, `--check`, `--stdout`, and the lexer refusing to
see models in comments and strings.

```
cargo test
```

## References

- RFC-0001 - What Cyclone is
- RFC-0002 - Binary Reader / Writer API, which names every method generated here
- RFC-0003 - Conformance

## License

Apache-2.0
