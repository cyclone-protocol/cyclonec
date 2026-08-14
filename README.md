# cyclonec

The official Cyclone **source generator**.

```text
source annotation  →  scanner/parser  →  model discovery  →  codec generation
```

`cyclonec` is not a compiler and not a runtime. It reads Cyclone attributes out
of your Rust, Go, C#, GDScript, C++, C, TypeScript or JavaScript sources -
`#[network]` / `#[codec(...)]`, Go's `//cyclone:model` directive and struct
tags, C#'s `[Network]` / `[Codec(...)]` attributes, GDScript's `# cyclone:model`
/ `# cyclone:TYPE` comment directives, C++/C's shared `CYCLONE_MODEL` /
`CYCLONE_CODEC(...)` / `CYCLONE_FIELD(TYPE)` macros, or TypeScript/JavaScript's
shared `// CYCLONE_MODEL` / `// CYCLONE_CODEC(...)` / `// CYCLONE_FIELD(TYPE)`
comment directives - and writes the `encode` / `decode` calls that go with
them, then exits, the way `protoc` does. One run reads one language; a project
with more than one gets one `cyclone.toml` (and one `--src`/`--out`) per
language.

What it writes reads and writes **your** types:

```rust
pub fn encode(writer: &mut Writer, value: &Player) {
    writer.write_u32(value.id);
    writer.write_f32(value.x);
}
```

```go
func (PlayerEdgeCodec) Encode(w *Writer, value *models.Player) {
	w.WriteU32(value.ID)
	w.WriteF32(value.X)
}
```

```csharp
public static void Encode(Writer writer, Models.Player value)
{
    writer.WriteU32(value.Id);
    writer.WriteF32(value.X);
}
```

```gdscript
static func encode(writer: CycloneRuntime.Writer, value: Player) -> void:
	writer.write_u32(value.id)
	writer.write_f32(value.x)
```

```cpp
static void encode(Writer& writer, const models::Player& value) {
    writer.write_u32(value.Id);
    writer.write_f32(value.X);
}
```

```c
static inline bool PlayerEdgeCodec_encode(CycloneWriter *writer, const struct Player *value) {
    if (!cyclone_writer_write_u32(writer, value->Id)) return false;
    if (!cyclone_writer_write_f32(writer, value->X)) return false;
    return true;
}
```

No `PlayerDTO`, no `PlayerWire`, no `PlayerMapper`, no runtime mapping layer, no
registry, no reflection. The bytes in between are RFC-0002's, produced by a
runtime block that is copied out unchanged rather than derived per model - so
the generator cannot invent a wire format even in principle.

Zero dependencies, in the generator and in what it generates.

---

## What a run produces

Rust, shown below; Go's, C#'s, GDScript's, C++'s and C's shapes are the same
idea in each language's own terms - see [Go](#go), [C#](#c), [GDScript](#gdscript),
[C++](#c-1) and [C](#c-2).

```text
cyclonec generate --src src --out generated
```

```text
src/generated/
    mod.rs             the module root: declares and re-exports the rest
    runtime.rs         the RFC-0002 runtime, verbatim
    handshake.rs       every fingerprint, and the handshake
    player_edge.rs     one codec, one file
    player_unity.rs
.cyclone/
    schema.json        the schema, as an artifact - commit this
    build-graph.json   which source produced which file
```

It is an ordinary Rust module tree - mount it the ordinary way:

```rust
mod generated;
use generated::{PlayerEdgeCodec, Reader, Writer};
```

No crate, nothing to add to `Cargo.toml`: the runtime is the `runtime` module
inside the tree, and `mod.rs` re-exports it along with every codec.

Each file is a module, so each file imports what it names - including your
model:

```rust
use super::runtime::{DecodeError, Reader, Writer};
use crate::models::player::Player;
```

Which leaves one thing the generator has to be told rather than assume: **where
your models live**. It reads that off the source layout, the same way Rust does
(`src/models/player.rs` is `crate::models::player`), and `model_path` overrides
it for a project whose modules do not mirror its directories, or one that
re-exports every model from a single place.

`cyclone.toml` in the project root saves typing the flags; a CLI flag always
wins over it.

```toml
src = "src"
out = "src/generated"
model_path = "crate::models"    # optional
```

Your models have to be reachable from the generated module: `pub struct`, with
the fields the codec touches visible to it.

---

## What it reads

Four markers, and nothing else in your file is inspected. Go has no attributes,
so the same four things are said with a comment directive and struct tags; C#
has attributes, so it says them the same way Rust does, just with C#'s own
syntax for one; GDScript has no attributes either - an unrecognized `@name` is
a parse error in Godot itself - so, like Go, it says them with a comment
directive too; C++ and C have no attribute syntax cyclonec can extend either
(and no comment-directive convention to fall back on the way Go and GDScript
do), so they say them with three macros a small header defines to expand to
nothing - the same header, and the same three macros, for both; TypeScript and
JavaScript have neither attributes nor macros usable without a decorator or a
runtime dependency (the brief this backend was built against forbids both), so
- like Go and GDScript - they say them with a comment directive, read the same
way for both languages:

| | Rust | Go | C# | GDScript | C++ / C | TypeScript / JavaScript |
|-|------|----|----|----------|-----|-----|
| this type is a model | `#[network]` | `//cyclone:model` | `[Network]` | `# cyclone:model` | `CYCLONE_MODEL` | `// CYCLONE_MODEL` |
| generate these codecs | `#[codec(edge, unity)]` | `//cyclone:model codec=edge,unity` | `[Codec("edge", "unity")]` | `# cyclone:model codec=edge,unity` | `CYCLONE_CODEC("edge", "unity")` | `// CYCLONE_CODEC("edge", "unity")` |
| this field's wire type | `#[network(u32)]` | `` `cyclone:"u32"` `` | `[Network("u32")]` | `# cyclone:u32` | `CYCLONE_FIELD(u32)` | `// CYCLONE_FIELD(u32)` |
| this field's codecs | `#[codec(edge)]` | `` `codec:"edge"` `` | `[Codec("edge")]` | `# cyclone:u32 codec=edge` | `CYCLONE_CODEC("edge")` | `// CYCLONE_CODEC("edge")` |

```rust
#[network]
#[codec(edge, unity)]
pub struct DeviceState {
    #[network(u32)]
    #[codec(edge, unity)]
    pub id: u32,

    #[network(f32)]
    #[codec(edge)]
    pub temperature: f32,

    #[network(string)]
    #[codec(unity)]
    pub display_name: String,

    /// A network field in no codec: written by none of them.
    #[network(u32)]
    pub unrouted: u32,

    /// Not on the wire at all.
    pub cache: String,
}
```

```go
//cyclone:model codec=edge,unity
type DeviceState struct {
	ID          uint32 `cyclone:"u32" codec:"edge,unity"`
	Temperature float32 `cyclone:"f32" codec:"edge"`
	DisplayName string `cyclone:"string" codec:"unity"`

	// A network field in no codec: written by none of them.
	Unrouted uint32 `cyclone:"u32"`

	// Not on the wire at all.
	Cache string
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

    [Network("f32")]
    [Codec("edge")]
    public float Temperature { get; set; }

    [Network("string")]
    [Codec("unity")]
    public string DisplayName { get; set; }

    /// A network field in no codec: written by none of them.
    [Network("u32")]
    public uint Unrouted { get; set; }

    /// Not on the wire at all.
    public string Cache { get; set; }
}
```

```gdscript
# cyclone:model codec=edge,unity
class_name DeviceState

# cyclone:u32 codec=edge,unity
var id: int = 0

# cyclone:f32 codec=edge
var temperature: float = 0.0

# cyclone:string codec=unity
var display_name: String = ""

# A network field in no codec: written by none of them.
# cyclone:u32
var unrouted: int = 0

# Not on the wire at all.
var cache: String = ""
```

```cpp
CYCLONE_MODEL
CYCLONE_CODEC("edge", "unity")
struct DeviceState
{
    CYCLONE_FIELD(u32)
    CYCLONE_CODEC("edge", "unity")
    uint32_t Id = 0;

    CYCLONE_FIELD(f32)
    CYCLONE_CODEC("edge")
    float Temperature = 0.0f;

    CYCLONE_FIELD(string)
    CYCLONE_CODEC("unity")
    std::string DisplayName;

    // A network field in no codec: written by none of them.
    CYCLONE_FIELD(u32)
    uint32_t Unrouted = 0;

    // Not on the wire at all.
    std::string Cache;
};
```

```c
CYCLONE_MODEL
CYCLONE_CODEC("edge", "unity")
struct DeviceState
{
    CYCLONE_FIELD(u32)
    CYCLONE_CODEC("edge", "unity")
    uint32_t Id;

    CYCLONE_FIELD(f32)
    CYCLONE_CODEC("edge")
    float Temperature;

    CYCLONE_FIELD(string)
    CYCLONE_CODEC("unity")
    const char *DisplayName;

    // A network field in no codec: written by none of them.
    CYCLONE_FIELD(u32)
    uint32_t Unrouted;

    // Not on the wire at all.
    int Cache;
};
```

```typescript
// CYCLONE_MODEL
// CYCLONE_CODEC("edge", "unity")
class DeviceState {
    // CYCLONE_FIELD(u32)
    // CYCLONE_CODEC("edge", "unity")
    Id: number = 0;

    // CYCLONE_FIELD(f32)
    // CYCLONE_CODEC("edge")
    Temperature: number = 0;

    // CYCLONE_FIELD(string)
    // CYCLONE_CODEC("unity")
    DisplayName: string = "";

    // A network field in no codec: written by none of them.
    // CYCLONE_FIELD(u32)
    Unrouted: number = 0;

    // Not on the wire at all.
    Cache: string = "";
}
```

JavaScript writes the identical directives, with every type annotation
dropped - `Id;` in place of `Id: number = 0;` - and means exactly the same
thing: the host type is never consulted in either language, only
`CYCLONE_FIELD`'s own argument is.

All eight generate exactly two codecs - `…EdgeCodec` (`id`/`ID`/`Id`,
`temperature`) and `…UnityCodec` (`id`/`ID`/`Id`, `display_name`) - never a
third, never one fewer.

**The wire type is never inferred from the host type.** `#[network(u32)]` (or
`` cyclone:"u32" ``) on a wider field is still four bytes, Little Endian.
Whether the host language accepts the resulting call is the host compiler's
question, not this generator's.

Each scanner is a lexer, not a parser for its language: it knows no types,
traits, generics, modules or packages. The one thing it must get right is
*where* a token is - a `#[` inside a string, or a `struct`/`type` inside a
comment, is not source.

`cyclonec` only *reads* those markers; the host compiler still has to accept
them. In Rust that means your crate needs `#[network]`/`#[codec]` defined - the
[`cyclone-attributes`](https://crates.io/crates/cyclone-attributes) crate, or
your own no-op equivalents (a dependency of your models, not of the generated
code: what `cyclonec` writes still depends on nothing). Go needs nothing extra
at all - a comment and a struct tag are already valid Go with no meaning to the
compiler until `cyclonec` reads them. C# needs a small `Network`/`Codec`
attribute pair defined somewhere your models can see - a few lines, no
dependency of their own - the same role `cyclone-attributes` plays for Rust.
GDScript needs nothing extra either, for the same reason Go doesn't: a
`# cyclone:` comment is already valid GDScript with no meaning to Godot's own
compiler until `cyclonec` reads it. C++ and C both need the same small header
defining `CYCLONE_MODEL`, `CYCLONE_FIELD` and `CYCLONE_CODEC` to expand to
nothing - again a dependency of your models, never of the generated code, and
the one file the two backends actually share. TypeScript and JavaScript need
nothing extra at all, for the same reason Go and GDScript don't: a
`// CYCLONE_...` comment is already valid source in both with no meaning to
`tsc`, a bundler, or Node until `cyclonec` reads it - no decorator, and no
package to install.

---

## Go

Go compiles by *package*, not by file, so the Go backend differs from the Rust
one in three ways:

- **No module root.** Every file `cyclonec` writes for one run shares a single
  `package` clause (derived from `--out`'s own directory name), so there is
  nothing to declare and nothing to re-export - a codec is reached the ordinary
  way, `generated.PlayerEdgeCodec{}`.
- **Import paths, not module paths.** A codec has to `import` the package your
  models live in. By default that is computed from the nearest `go.mod`'s
  `module` line plus the model source's own directory - `go.mod` therefore has
  to sit at the project root, next to `cyclone.toml`. `--model-path` overrides
  it (an import path, e.g. `github.com/acme/game/internal/models`, not a Rust
  module path) for a layout that does not fit that assumption.
- **`error`, not `Result`.** `Decode` returns `error` and every read is
  followed by an explicit check, the shape Go itself asks for; the RFC-0002
  §9.1 policy is identical - `r.FieldAbsent()` at every field boundary, `error`
  only for a field that started and ran out.

`Array<Array<T>>` is refused with a clear error rather than generated wrong -
the one thing the Go backend does not yet support. Flatten the field, or split
it into two codecs.

---

## C#

C# compiles by *project*, not by file or by package, so the C# backend differs
from both of the others:

- **No `import`, ever.** A fully-qualified reference (`Models.Player`)
  compiles without a `using` directive, so a generated codec never writes one
  - it spells a cross-namespace reference out in full, and a bare reference
  whenever the model shares this run's namespace (derived from `--out`'s own
  directory name, PascalCased) or has no namespace at all.
- **`namespace`, not an import path.** Where each model lives is read from the
  `namespace` its own source declares (or no namespace, C#'s global one, which
  is a valid answer) - `--model-path` overrides every model at once with a
  namespace of your choosing, the same role it plays for Go's import path.
- **Exceptions, not `Result`/`error`.** `Decode` takes `ref Reader` and throws
  `DecodeException` on failure; the RFC-0002 §9.1 policy is identical -
  `reader.FieldAbsent()` at every field boundary, throwing only for a field
  that started and ran out.
- **A local, not a borrow, for a nested model.** A C# property cannot be
  passed `ref` directly, so a nested model field is decoded through a local:
  read the existing value out, decode into it by `ref`, assign it back. This
  needs the field to already hold an instance before `Decode` runs (a property
  initializer, `= new();`, is enough) - the same requirement Rust's version
  has, enforced by the language there and by the caller here.

`Array<Array<T>>` is refused for the same reason it is in Go: flatten the
field, or split it into two codecs.

---

## GDScript

GDScript compiles by *file*, and a `.gd` file gets exactly one globally
reachable name: whatever it declares with `class_name`. That single fact
shapes the whole backend, and turns out to fit this project's "one file per
model per codec" layout better than `cyclonec_old`'s one shared file ever
could:

- **No `import`, and no shared wrapper.** Every codec file declares its own
  `class_name` - `PlayerEdgeCodec`, say - and Godot makes it reachable
  project-wide with nothing to `preload`. A model reference is always bare:
  there is no namespace, no package and nothing to qualify, so unlike Go and
  C# this backend has no `Imports` concept at all. `--model-path` has no
  effect here, because there is nothing for it to override.
- **`static func`, not an instance method.** `encode` and `decode` are called
  directly - `PlayerEdgeCodec.encode(writer, value)` - with nothing to
  `.new()`. (`cyclonec_old` instantiated every codec instead, to avoid an
  unresolved question about `static func` *inside a nested class*; that
  question doesn't apply once a codec is its own top-level file.)
- **`[value, error]`, not `Result`/exceptions.** GDScript has no
  `try`/`catch`, so every `Reader` read returns a 2-element `Array` instead of
  throwing or returning `Result`, and `decode` returns a `DecodeError` (or
  `null`, on success) rather than throwing one - the same explicit-check shape
  Go's `error` return already gives this project. The RFC-0002 §9.1 policy is
  identical: `reader.field_absent()` at every field boundary, an error only
  for a field that started and ran out.
- **A signed 64-bit `int`, and no `u64`.** A fingerprint is an opaque 64-bit
  bit pattern, and GDScript's only integer type is signed - so every
  fingerprint constant is built from two 32-bit halves shifted and combined,
  rather than risking a bare 16-digit hex literal a signed parser was never
  asked to handle.

`Array<Array<T>>` is refused for the same reason it is in Go and C#: flatten
the field, or split it into two codecs. And unlike the other three backends,
this one cannot be compiled or run in this project's own CI - there is no
official headless Godot GitHub Action to build against, so only the half
`cyclonec` itself can verify (parsing, generation, `--check`) is checked
automatically; open `tests/fixtures-gd/` in the Godot editor to check the rest
by hand.

---

## C++

C++ compiles by *translation unit*, not by package or by project, and this
backend is header-only for exactly that reason: every method a generated
`struct` declares is defined inside the struct body, which makes it
implicitly `inline` and safe to `#include` from as many `.cpp` files as a
project likes, with no separate compilation unit to add to a build.

- **A physical `#include`, not only a name in scope.** Unlike C#'s "a
  fully-qualified name compiles with no `using`" or Go's "one `import` reaches
  a whole package", a C++ codec needs the header that declares its model
  named explicitly. That path is always the model's own source path exactly
  as `--src` found it (e.g. `src/models/player.hpp`) - point your compiler's
  `-I` at whatever directory that path is itself relative to (typically the
  project root) and it resolves. `--model-path` overrides the *namespace* a
  model is qualified with, the same as it does for C#, but never this path:
  the physical header a build needs is not something a logical override can
  change.
- **Always a leading `::`.** A namespaced model is spelled
  `::Game::Models::Player`, always fully qualified from the global namespace -
  never bare-if-same-namespace the way Go's and C#'s `qualify` have to be.
  C++ has no rule against a self-referential qualified name, so there is no
  "is this the same namespace as the one this file opens" case to get right,
  and [`generator::cpp`](src/generator/cpp.rs) tracks no "own namespace" at
  all.
- **A `DecodeError` return, not an exception.** A generated model's fields are
  plain public members, never properties, so - unlike C# - a nested model's
  field is always addressable and passed by reference directly, with none of
  C#'s `var local = value.Field; …; value.Field = local;` workaround needed.
  And rather than throw the way C#'s runtime does, every `Reader` read takes
  its result by output reference and returns a `DecodeError` - a
  default-constructed one *is* "no error" - so a project built with
  exceptions off (`-fno-exceptions`, common in game and embedded C++) can
  still call it. The RFC-0002 §9.1 policy is identical:
  `reader.field_absent()` at every field boundary, an error only for a field
  that started and ran out.
- **No 64-bit workaround needed.** Unlike GDScript, C++'s `std::uint64_t` is
  exact and unsigned, and an unsuffixed hex literal too wide for `int`/`long`
  is promoted to `unsigned long long` by the language itself - so a
  fingerprint is written as a plain `0x…ULL` literal, no splitting required.

`Array<Array<T>>` is refused for the same reason it is in Go, C# and GDScript:
flatten the field, or split it into two codecs. Unlike GDScript, this backend
*is* compiled - and its generated tree actually run, round-tripping a real
payload - in this project's own CI: g++ ships on the standard runner image, so
`tests/fixtures-cpp/` gets the same rigor `tests/generated.rs` gives the Rust
tree, via a small hand-written smoke test (`tests/fixtures-cpp/smoke_test.cpp`)
rather than a second Rust integration test. Generated code targets C++17 (for
`inline` namespace-scope constants in `handshake.hpp`) and nothing later.

---

## C

Plain C reads the same `CYCLONE_MODEL`/`CYCLONE_CODEC`/`CYCLONE_FIELD` markers,
from the same header, as C++ - but has none of C++'s classes, namespaces,
references, exceptions or growable containers, so this backend's generated
shape departs from C++'s in every place those are what C++ leaned on:

- **Free functions, not static methods.** Where C++ writes `struct
  PlayerEdgeCodec { static bool encode(...); };`, C writes two ordinary
  functions, `PlayerEdgeCodec_encode` and `PlayerEdgeCodec_decode` - both
  `static inline`, the same header-only, multi-translation-unit-safe shape
  every function this backend generates has (including the runtime's own).
- **`struct Name`, always - never bare `Name`.** The brief's own `DeviceState`
  example above declares a plain tagged `struct DeviceState { ... };`, no
  `typedef` - so bare `DeviceState` is not a C type at all, only `struct
  DeviceState` is. Every generated reference to a model spells it that way,
  which compiles whether or not your project *also* writes a `typedef struct
  DeviceState DeviceState;` alongside it.
- **No namespace, so nothing for `--model-path` to override.** A model's type
  is reached by the same physical `#include` C++ needs (always the model's own
  source path, e.g. `src/models/player.h` - point your compiler's `-I` at
  whatever it's relative to) and nothing else: there is no logical
  qualification step at all, so unlike C++ this backend has no `Imports`
  concept beyond that one `#include` lookup, and `--model-path` has no effect
  on it, the same as GDScript.
- **`CycloneDecodeError` returned by value, and `bool` for a fallible write.**
  Every `Reader` read takes its result by output pointer and returns a
  `CycloneDecodeError` (a zero-initialized one *is* "no error"), the same
  no-exceptions shape C++ uses. Encoding can fail too, though - `malloc`
  clearly indicates failure through `NULL`, and C has no `std::vector` whose
  reallocation just throws - so every generated `_encode` and every
  `CycloneWriter` method returns `bool`, checked the same way a decode error
  is: `if (!cyclone_writer_write_u32(writer, value->Id)) return false;`.
- **`string`/`bytes`/`Array<T>` fields are heap-owned, and you free them.** A
  `string` field's host type is always `const char *`, heap-allocated by
  decode and released by `free()`; `bytes` decodes into a `CycloneBytes {
  data, len }`; `Array<T>` decodes into a generated `CycloneArray_T { items,
  count }` - one small owned type per *distinct* `T` the schema actually
  uses, written once into a shared `arrays.h`, since C has no generic
  container to reach for. Every model gets one more generated file,
  `<model>_cyclone.h`, carrying a single `<Model>_free` that walks every
  field any of that model's codecs ever decodes and releases what it owns -
  call it exactly once per decoded value, and only ever decode into a struct
  that is freshly zero-initialized or freshly freed (decode does not free
  what a field already held before writing over it - see `runtime.h`'s
  module docs for why that would be its own bug).

`Array<Array<T>>` is refused for the same reason it is everywhere but Rust:
flatten the field, or split it into two codecs. Like C++, this backend *is*
compiled and run in this project's own CI - gcc ships on the same runner image
- via a small hand-written smoke test (`tests/fixtures-c/smoke_test.c`).
Generated code targets C99 (for `//` comments, `inline`, mixed declarations,
and compound literals) and nothing later.

---

## TypeScript

TypeScript needs no external project file the way Go needs `go.mod` - a
generated codec reaches your model class through an ordinary ES `import`,
computed straight from the model's own source path, the same "no project file
needed" simplicity GDScript has but with real per-file paths instead of one
global namespace:

- **A `class`, generated against directly - never a DTO.** `encode`/`decode`
  are `static` methods on a generated `PlayerEdgeCodec`, taking and mutating
  the exact class your source declares:
  `PlayerEdgeCodec.encode(writer, value)`, `PlayerEdgeCodec.decode(reader,
  value)`. Nothing is ever copied into an intermediate shape.
- **`bigint`, not `number`, for `i64`/`u64`.** A JS `number` is an IEEE 754
  double, exact only up to 2^53 - short of a full 64-bit range - so `i64` and
  `u64` fields, and every fingerprint and per-frame envelope value, are
  `bigint` here and nowhere else. Every other primitive maps the way you would
  expect: `u32`/`i32`/`f32`/`f64`/small integers all to `number`, `string` to
  `string`, `bool` to `boolean`, `bytes` to `Uint8Array`.
- **A relative `import`, computed from your source layout.** By default a
  generated codec's `import` is a relative path from `--out` to the model's
  own source file - `src/generated/player_edge.ts` importing `Player` from
  `src/models/player.ts` writes `import { Player } from
  "../models/player";`. `--model-path` overrides it with one shared module
  specifier for every model at once (a barrel file re-exporting each of
  them), the same "one string overrides every model" meaning it has for Go's
  import path and C#'s namespace.
- **A nested model is constructed with `new` if it is not already there.**
  Unlike a Rust or Go struct field, nothing guarantees a TypeScript class
  field already holds an instance before `decode` reaches it - so a bare
  nested-model field (and each array-of-model element) is constructed with
  `new ModelName()` first. This needs the nested class to have a public,
  parameterless constructor - the same requirement Rust's version has (a
  nested model must implement `Default`), enforced by the language there and
  by the caller here.
- **Exceptions, not `Result`.** `decode` throws `DecodeError` on failure; the
  RFC-0002 §9.1 policy is identical - `reader.fieldAbsent()` at every field
  boundary, throwing only for a field that started and ran out.

`Array<Array<T>>` is refused for the same reason it is everywhere but Rust.
This backend *is* compiled and run in this project's own CI, with `tsc`
installed from the fixture's own `package.json`, via a small hand-written
smoke test (`tests/fixtures-ts/smoke_test.ts`).

---

## JavaScript

The JavaScript backend is TypeScript's, with every type annotation erased -
`@param`/`@returns` JSDoc comments in their place - and one difference that is
not cosmetic: **the generated file is meant to be run directly**, by Node's
own ESM loader or a browser's, neither of which resolves an extensionless
relative specifier the way `tsc` or a bundler would. So every `import` this
backend writes carries an explicit `.js` extension
(`import { Player } from "../models/player.js";`), where TypeScript's does
not.

A JavaScript codec file also imports less than its TypeScript counterpart: a
JS function parameter carries no type, so the model a codec *belongs to* is
never imported at all (nothing in `PlayerEdgeCodec` ever spells `Player` by
name) - only a model this file actually constructs with `new` (a nested field,
or an array of them) is. Otherwise every rule TypeScript's section above
states - the `bigint` mapping, the relative-import computation, constructing
an absent nested model, throwing `DecodeError`, refusing `Array<Array<T>>` -
applies unchanged, because both backends walk the identical IR message (see
`src/ir.rs`); only the surface syntax differs. This backend needs no build
step in CI at all - its committed fixture is simply run with `node`
(`tests/fixtures-js/smoke_test.js`).

---

## Decoding, and version skew

RFC-0002 §9.1 says a byte stream that ends **exactly on a field boundary** is
valid: the writer was running an older model, and the reader's remaining fields
are simply absent. A stream that ends **inside** a field is a truncated packet
and must be an error.

Telling those apart is the whole of it, and the generated decoder asks one
question per field:

```rust
value.level = if reader.field_absent() { 0u32 } else { reader.read_u32()? };
```

| the stream | the field | result |
|---|---|---|
| ends before the field starts | absent | zero, and so is every field after it |
| ends two bytes into a `u32` | truncated | `DecodeError::UnexpectedEof` |
| has bytes left after the last field | a newer writer's | ignored |

A partial field is **never** treated as a zero. Packet corruption that decodes to
a plausible value is worse than packet corruption that fails.

Array *elements* are read strictly: a count of 3 promised three elements, so a
stream that ends after two is a truncated array, not skew.

A nested model follows the same rule at its own level, without any code of its
own - its codec asks the same question of each of its own fields.

> `cyclonec_old` could not do this: every read returned `UnexpectedEof`, so
> version skew failed in both directions. See [MIGRATION.md](MIGRATION.md) §1.1.

---

## Fingerprints

Every message - a model rendered by one codec - has a fingerprint: SHA-256 over
a fully specified canonical text, so that Rust, Go, C#, C++, C, TypeScript and
JavaScript produce the same 32 bytes from the same schema (see
`tests/cross_language.rs`, which checks exactly that). `handshake.rs` publishes
them:

```rust
pub const CYCLONE_SCHEMA_FINGERPRINT: u64 = 0x6D1B58906FA09FFA;

pub const PLAYER_FINGERPRINT: u64 = 0xB1C59A2609840A9F;
pub const PLAYER_EDGE_MESSAGE_ID: u32 = 0x432AB486;
pub const PLAYER_EDGE_FINGERPRINT: u64 = 0x231DD2D8744FECC3;
```

Generated, never hand-maintained: a constant a human keeps in step with a schema
is one commit away from being wrong, and a wrong fingerprint says *current*
about two peers that disagree.

A fingerprint answers **same or different**, and nothing else. It changes for
every change in the table in [SPEC-FINGERPRINT.md](SPEC-FINGERPRINT.md) §6 -
including two same-typed fields being swapped, which is why field names are part
of the canonical input. That document is normative and explains the trade-off in
full.

A message **id** is derived from the message name alone, so appending a field
does not renumber it: a peer can still say *which* message it means while
disagreeing about its shape.

---

## Handshake

```text
Client  ──  CYCLONE_SCHEMA_FINGERPRINT  ──▶  Server
```

| | |
|---|---|
| the same schema fingerprint | `CURRENT`, accept |
| a message both ends know, with different fingerprints | `REJECT`, disconnect |
| otherwise - each end knows messages the other does not | `OUTDATED`, accept |

```rust
match cyclone_handshake(peer_schema_fingerprint, peer_messages) {
    CycloneHandshake::Current => accept(),
    CycloneHandshake::Outdated => accept(),   // one side is behind, safely
    CycloneHandshake::Reject => disconnect(),
}
```

`peer_messages` is the peer's `(id, fingerprint)` table - what `CYCLONE_MESSAGES`
is on this side. **No schema crosses the network.** Both ends have theirs
compiled in, and sending one would invite a peer to interpret it, which is the
runtime schema resolution Cyclone exists to not have.

The middle rule is the safety property, and it is the most a runtime can know:
two peers that both speak `Player.edge` and disagree about its bytes must not
exchange it. *How* they disagree is a build-time question, answered from two
schemas by `cyclonec compat`.

No frame carries a fingerprint by default - the wire's premise is that there is
no metadata on it. A project that wants per-frame validation can turn it on:

```toml
validate_message_fingerprint = true
```

and every frame gains `[MessageId: u32][MessageFingerprint: u64]` in front of
its payload, with `cyclone_write_envelope` / `cyclone_read_envelope` generated to
match.

---

## Schema evolution

`.cyclone/schema.json` is the schema as an artifact: for inspecting a build, for
`cyclone-inspect`, for CI, and as the baseline the next change is compared
against. **It is never a runtime dependency, and never an input to generation.**
Every run re-derives the schema from source; the file on disk is the previous
answer, kept so the new one can be compared against it.

```text
Source Model
      ↓
Scanner / Parser
      ↓
Cyclone IR  ────┬───────────────→ generated codec
                ├───────────────→ schema.json
                ├───────────────→ fingerprints
                └───────────────→ build-graph
```

### What counts as breaking

| Change | Verdict |
|---|---|
| nothing changed | `CURRENT` |
| a field appended at the end | `COMPATIBLE` |
| a field removed, from anywhere | `BREAKING` |
| a field inserted in the middle | `BREAKING` |
| fields reordered | `BREAKING` |
| a field's wire type changed | `BREAKING` |
| a field renamed, nothing else | `BREAKING` - the bytes are unchanged, the fingerprint is not |
| a whole message added | `COMPATIBLE` |
| a whole message removed | `BREAKING` |

The fingerprint is not asked which of these it was - it has no structure to
infer from. When two fingerprints differ, the checker compares the two schemas
field by field and says exactly what moved:

```text
⚠ Player.edge:
  field[1]:
    old: x:f32
    new: y:f32
  BREAKING: field order changed
```

```text
⚠ Player.edge:
  + level:u32 at index 3
  COMPATIBLE: append-only fields (1 appended at the end)
```

### Locally: a warning, never a failure

`cyclonec generate` prints the report and generates anyway. Breaking a schema on
a branch is a decision a developer is allowed to make, and a generator that
refused would only teach them to pass a flag that turns the check off for good.

### In CI: an error

```bash
cyclonec ci --base-ref "origin/${GITHUB_BASE_REF}"
```

1. `.cyclone/schema.json` still matches this branch's source - otherwise every
   comparison after it is against fiction;
2. the **target branch's** `schema.json`, read out of git rather than the working
   tree;
3. the two compared: `BREAKING` is exit 1, `CURRENT` / `COMPATIBLE` is exit 0.

The baseline is always given and never defaulted. A baseline hard-coded to
`main` in a repository that merges into `develop` produces a green check that
compared a branch against itself. See
[`.github/workflows/schema.yml`](.github/workflows/schema.yml).

---

## cyclone-inspect

```bash
cyclone-inspect --schema .cyclone/schema.json --message Player --file packet.bin
cyclone-inspect --schema .cyclone/schema.json --message Player.edge --hex '64000000 00002841 0000a041'
```

```text
Player.edge
fingerprint: sha256:231dd2d8744fecc3198c9853ffafe18023c93670fe7822c4cd9638fe9eabbe8b (0x231DD2D8744FECC3)
message id : 0x432AB486
payload    : 12 bytes
----------------------------------------------------
id      : u32 = 100
          offset: 0
          bytes: 64 00 00 00

x       : f32 = 10.5
          offset: 4
          bytes: 00 00 28 41
```

The schema is **named, never guessed**. There is no tag, no id and no length in
front of a Cyclone payload to infer a message from, and a plausible-looking
wrong answer is worse than no answer. `--expect <sha256:… | 0x…>` fails unless
the message's fingerprint is the one you expected, so a packet captured from one
build cannot be quietly read through another.

It decodes by exactly the rules a generated decoder follows: an absent field is
reported as absent, a truncated field is an error, and trailing bytes are
reported as a newer writer's.

---

## The build graph

`.cyclone/build-graph.json` maps each source to what was generated from it, with
the message fingerprint and the SHA-256 of the bytes written:

```json
{
  "sources": {
    "src/models/player.rs": {
      "models": ["Player", "PlayerInfo"],
      "outputs": [
        {
          "path": "src/generated/player_edge.rs",
          "model": "Player",
          "codec": "edge",
          "fingerprint": "sha256:231dd2…",
          "sha256": "9f1e35…"
        }
      ]
    }
  }
}
```

It answers two questions nothing else can: *where did this file come from* (even
after its source was deleted) and *is this file stale* (a digest that no longer
matches means somebody edited it by hand, and the header did say not to). It is
also how `cyclonec generate` knows to delete the codec of a model you removed.

---

## Commands

```text
cyclonec generate [--src <PATH>]... [--out <PATH>] [--model-path <PATH>] [--check] [-q]
cyclonec compat --base <SCHEMA> [--head <SCHEMA>]
cyclonec ci --base-ref <REF>
cyclone-inspect --schema <SCHEMA> --message <NAME> (--file <PATH> | --hex <HEX>)
```

| | |
|---|---|
| `generate` | read source, report what changed, write the tree |
| `generate --check` | write nothing; exit 1 if anything on disk is out of date |
| `compat` | compare a base schema against the current source, or against `--head`; exit 1 on `BREAKING` |
| `ci` | verify, fetch the target branch's schema, compare, exit 1 on `BREAKING` |

There is deliberately no `--codec` flag: a model declares its codecs in the
source, and asking again on the command line could only ever disagree.

---

## Layout

```text
cyclonec/
├── src/
│   ├── bin/
│   │   ├── cyclonec.rs         generate / compat / ci
│   │   └── cyclone_inspect.rs
│   ├── cli.rs                  three commands, parsed by hand
│   ├── config.rs               cyclone.toml
│   ├── gomod.rs                 just enough of go.mod to compute an import path
│   ├── parser/
│   │   ├── rust.rs             lexer + scanner for #[network] / #[codec(...)]
│   │   ├── go.rs               lexer + scanner for //cyclone:model + struct tags
│   │   ├── csharp.rs           lexer + scanner for [Network] / [Codec(...)]
│   │   ├── gdscript.rs         scanner for # cyclone:model / # cyclone:TYPE comments
│   │   ├── cpp.rs              lexer + scanner for CYCLONE_MODEL / CYCLONE_CODEC(...) / CYCLONE_FIELD(...)
│   │   └── c.rs                the same, minus namespace/class/access-specifier handling
│   ├── model.rs                what the scanner collected
│   ├── ir.rs                   the Cyclone IR - the source of truth
│   ├── fingerprint.rs          the canonical form, and SHA-256 over it
│   ├── schema.rs               .cyclone/schema.json
│   ├── compat.rs               CURRENT / COMPATIBLE / BREAKING
│   ├── buildgraph.rs           .cyclone/build-graph.json
│   ├── generate.rs             discover → parse → IR → render → compare → write
│   ├── generator/
│   │   ├── rust.rs             one message → one file
│   │   ├── rust_runtime.rs     the RFC-0002 block, as one constant
│   │   ├── handshake.rs        the fingerprint constants, Rust
│   │   ├── go.rs               one message → one file, Go
│   │   ├── go_runtime.rs       the RFC-0002 block, as one constant, Go
│   │   ├── go_handshake.rs     the fingerprint constants, Go
│   │   ├── csharp.rs           one message → one file, C#
│   │   ├── csharp_runtime.rs   the RFC-0002 block, as one constant, C#
│   │   ├── csharp_handshake.rs the fingerprint constants, C#
│   │   ├── gdscript.rs         one message → one file, GDScript
│   │   ├── gdscript_runtime.rs the RFC-0002 block, as one constant, GDScript
│   │   ├── gdscript_handshake.rs the fingerprint constants, GDScript
│   │   ├── cpp.rs              one message → one header, C++
│   │   ├── cpp_runtime.rs      the RFC-0002 block, as one constant, C++
│   │   ├── cpp_handshake.rs    the fingerprint constants, C++
│   │   ├── c.rs                one message → one header (free functions), C
│   │   ├── c_runtime.rs        the RFC-0002 block, as one constant, C
│   │   └── c_handshake.rs      the fingerprint constants, C
│   ├── inspect.rs              cyclone-inspect
│   ├── json.rs                 written by hand: key order is authored
│   ├── sha256.rs               written by hand: a hash must not have a version
│   └── timestamp.rs
├── tests/
│   ├── cli.rs                  the real binaries, over real files
│   ├── generated.rs            the committed Rust generated tree, compiled and run
│   ├── vectors.rs              the cross-SDK vectors, checked
│   ├── fixtures/               a small Rust project, laid out like a real one:
│   │                           src/models/*.rs annotated in place, and the
│   │                           src/generated/ tree written from them
│   ├── fixtures-go/            the same, in Go - go.mod, src/models/*.go,
│   │                           src/generated/*.go; built and `go vet`ted in CI
│   │                           (.github/workflows/ci.yml), since cargo test has
│   │                           no Go toolchain to compile it with
│   ├── fixtures-cs/            the same, in C# - a .csproj, src/models/*.cs,
│   │                           src/generated/*.cs; built in CI
│   │                           (.github/workflows/ci.yml), since cargo test has
│   │                           no .NET SDK to compile it with
│   ├── fixtures-gd/            the same, in GDScript - src/models/*.gd (one
│   │                           model per file), src/generated/*.gd; only
│   │                           `generate --check`ed in CI, never built - see
│   │                           the GDScript section above
│   ├── fixtures-cpp/           the same, in C++ - include/cyclone.h,
│   │                           src/models/player.hpp, src/generated/*.hpp;
│   │                           built with g++ and its smoke test actually run
│   │                           in CI (.github/workflows/ci.yml), since cargo
│   │                           test has no C++ toolchain to compile it with
│   ├── fixtures-c/             the same, in C - include/cyclone.h (shared
│   │                           with the C++ fixture), src/models/player.h,
│   │                           src/generated/*.h; built with gcc and its
│   │                           smoke test actually run in CI, for the same
│   │                           reason as the C++ fixture
│   ├── fixtures-ts/            the same, in TypeScript - package.json,
│   │                           tsconfig.json, src/models/*.ts,
│   │                           src/generated/*.ts; built with tsc and its
│   │                           smoke test actually run in CI
│   ├── fixtures-js/            the same, in JavaScript - src/models/*.js,
│   │                           src/generated/*.js; no build step - its
│   │                           smoke test is run directly with node
│   ├── cross_language.rs       one schema, read through every parser, proving
│   │                           every language fingerprints it identically
│   └── vectors/cyclone-vectors.json
├── SPEC-FINGERPRINT.md         normative: the canonical form
└── MIGRATION.md                what changed from cyclonec_old, and why
```

A further target language is a `parser/<lang>.rs` and a `generator/<lang>.rs` +
`generator/<lang>_runtime.rs` pair - see [`parser/go.rs`](src/parser/go.rs) and
[`generator/go.rs`](src/generator/go.rs) (or their C#, GDScript, C++, C,
TypeScript and JavaScript counterparts) for what that looked like the last
seven times. Nothing above `ir.rs` moved: schema, fingerprints, compatibility
and the build graph are language-independent by construction, exactly as
designed.

---

## Tests

```bash
cargo test
```

- **`src/**`** - unit tests: the scanner, the IR and its checks, the canonical
  fingerprint text and a pinned digest, every row of the compatibility table,
  the JSON round trip, SHA-256 against its published vectors.
- **`tests/generated.rs`** - the committed `tests/fixtures/src/generated/` tree
  compiled into a real crate and run, against the **same annotated model files**
  `cyclonec` scanned - not a second copy of them. Every byte expectation is read off
  RFC-0002: endianness, `-0.0` keeping its sign, a `string` length counted in
  bytes, a nested model inline, and §9.1 line by line - trailing bytes, an
  absent field, a partial field, a truncated array. Plus the three handshake
  outcomes.
- **`tests/cli.rs`** - the real binaries over real files: what gets written and
  where, `--check`, the compatibility warnings, `compat`'s exit codes, `ci`
  against a real git repository with a `develop` branch, the inspector, that a
  stale `schema.json` changes what is *reported* and never what is generated,
  the Go backend - one file per codec in a shared package, `Array<Array<T>>`
  refused, a mixed Rust/Go `--src` refused, `go.mod` required at the root -
  the C# backend - one file per codec in a shared namespace, `--model-path`
  overriding it, a mixed Rust/C# `--src` refused - the GDScript backend -
  one file per codec, no qualification of any kind, `--model-path` proven to
  have no effect, a mixed Rust/GDScript `--src` refused - the C++
  backend - one file per codec in a shared namespace, the model header
  `#include`d by its own source path, `--model-path` overriding the
  namespace but never that `#include` path, a mixed Rust/C++ `--src`
  refused - the C backend - one file per codec plus one free-function
  file per model, the model header `#include`d by its own source path,
  `--model-path` proven to have no effect (there is no namespace to
  override), a mixed Rust/C `--src` refused - the TypeScript backend -
  one file per codec, a relative `import` computed from the model's own
  source path, `--model-path` overriding it with one shared specifier, the
  brief's own `DeviceState` example parsing and generating correctly, every
  invalid-annotation example from the brief reported rather than silently
  guessed at, `Array<Array<T>>` refused, a mixed Rust/TypeScript `--src`
  refused - and the JavaScript backend - the same shape, plus a mixed
  TypeScript/JavaScript `--src` refused (the two share one annotation
  concept, but are still two languages as far as `--src`/`--out` is
  concerned).
- **`tests/cross_language.rs`** - the brief's own `DeviceState` example, and a
  second schema covering every primitive, an array and a nested model,
  parsed through the Rust, TypeScript, JavaScript, Go and C# scanners and
  built into a schema each: every one fingerprints identically, proving
  cross-language compatibility is a property of the IR (see
  `src/fingerprint.rs`) rather than something any one backend has to get
  right on its own.
- **`tests/vectors.rs`** - `tests/vectors/cyclone-vectors.json`, the artifact
  another SDK checks itself against: fixed bytes and fixed digests, verified
  through the real generated codecs.
- **`tests/fixtures-go/` and `tests/fixtures-cs/`, in CI, not `cargo test`** -
  `cargo test` has neither a Go toolchain nor a .NET SDK, so
  `.github/workflows/ci.yml` builds (and, for Go, `go vet`s) each committed
  fixture directly: real compilation, the same rigor `tests/generated.rs` gets
  from `rustc` for the Rust one.
- **`tests/fixtures-gd/`, in CI, `generate --check` only** - there is no
  headless Godot toolchain in this project's CI at all, official or
  otherwise, so unlike the Go and C# fixtures, this one is never actually
  compiled by anything this repository runs; only that the committed tree is
  what `cyclonec` writes today is verified automatically.
- **`tests/fixtures-cpp/`, in CI, built *and* run** - `cargo test` has no C++
  toolchain either, but g++ ships on CI's own runner image, so
  `.github/workflows/ci.yml` goes one step further here than it does for Go
  or C#: it compiles every generated header under `-Wall -Wextra -Wpedantic
  -Werror`, then actually executes `smoke_test.cpp` - a hand-written program,
  not part of the generated tree - which encodes, decodes, and checks RFC-0002
  §9.1's version-skew and truncation cases and the three handshake outcomes
  against real compiled output.
- **`tests/fixtures-c/`, in CI, built *and* run** - the same treatment as the
  C++ fixture, with gcc in place of g++ and `-std=c99` in place of
  `-std=c++17`: `smoke_test.c` round-trips a `Team` with a nested model, a
  `string`/`bytes` field and three kinds of `Array<T>`, exercises §9.1's
  version-skew and truncation cases, calls the generated `_free` functions,
  and checks the three handshake outcomes, all against real compiled output.
- **`tests/fixtures-ts/`, in CI, built *and* run** - `cargo test` has no
  TypeScript toolchain either; `.github/workflows/ci.yml` installs `tsc` from
  the fixture's own `package.json`, type-checks every generated file under
  `strict`, compiles to CommonJS, and runs `smoke_test.ts`: every primitive
  at its RFC-0002 width (including a `bigint` `u64`/`i64` round trip), a
  nested model, three kinds of `Array<T>`, §9.1's version-skew and truncation
  cases, and the handshake, all against real compiled output.
- **`tests/fixtures-js/`, in CI, run - no build step at all** - the generated
  tree is plain ES modules, so `smoke_test.js` (the same checks as the
  TypeScript fixture's) is simply run with `node`, proving the generated
  code needs nothing installed to work.

---

## References

- RFC-0001 - What Cyclone is
- RFC-0002 - Wire format, which names every method the runtime carries
- RFC-0003 - Conformance
- [SPEC-FINGERPRINT.md](SPEC-FINGERPRINT.md) - the fingerprint canonical form
- [MIGRATION.md](MIGRATION.md) - from `cyclonec_old`

## License

Apache-2.0
