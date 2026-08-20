# From `cyclonec_old` to `cyclonec` 0.2

The rewrite keeps Cyclone's philosophy and mechanism intact:

```text
source annotation  →  scanner/parser  →  model discovery  →  codec generation
```

Same attributes, same lexer, same field routing, same PascalCase codec names,
same runtime, **same bytes on the wire**. What is new is everything downstream of
the codecs: an IR, a schema artifact, fingerprints, a compatibility checker, a
build graph and a packet inspector.

This document is the honest list of what is *not* the same, and - as the brief
requires - it states where the old behaviour **contradicted** the specification
or the new requirements, before changing it.

---

## 1. Contradictions found in the old behaviour

### 1.1 The decoder rejected valid byte streams (RFC-0002 §9.1)

This is the one that mattered.

`cyclonec_old` generated, for every field:

```rust
value.level = reader.read_u32()?;
```

and its `Reader::take` returned `UnexpectedEof` whenever fewer bytes remained
than the read needed. There was no third answer, so a decoder could not tell
these two cases apart:

```text
the stream ended exactly on this field's boundary   → §9.1: valid, field absent
the stream ended two bytes into this field          → §10:  invalid, truncated
```

RFC-0002 §9.1 is explicit that the first is **not an error**:

> Bytes run out exactly on a field boundary → stop reading the Model there; the
> remaining fields are treated as absent.

So an old peer sending a three-field `Player` to a new peer expecting four
fields produced a decode *error*, when the Specification says it must produce a
`Player` with a zeroed fourth field. Version skew - the entire compatibility
story Cyclone has - did not work in the generated decoder, in either direction.

**Changed.** `Reader` gained one method, and the generated decoder one question
per field:

```rust
value.level = if reader.field_absent() { 0u32 } else { reader.read_u32()? };
```

`field_absent()` is `remaining() == 0`, asked only at a field boundary. Absence
zeroes the field and - since no read happens - every field after it. Truncation
still fails, exactly as before, with the same `UnexpectedEof`.

What this changes for existing code: byte streams that used to be **rejected**
now decode. Nothing that used to decode decodes differently, and no encoder
output changed by a single byte.

Array *elements* are read strictly, with no absence check: a count of 3 promised
three elements, and a stream that ends after two is a truncated array, not
version skew.

### 1.2 "It builds no schema, no IR" is no longer true, on purpose

`cyclonec_old`'s README made a virtue of having no IR: *"It builds no schema, no
IR, no type graph… and makes no second pass."*

Fingerprints, schema evolution and compatibility checking cannot be done without
one - a fingerprint over a nested model has to see that model, and a
compatibility report has to compare two schemas as structures. So there is now
an IR ([`src/ir.rs`](src/ir.rs)), and it is the source of truth for a run.

The part of that stance that was actually load-bearing is kept intact, and
tested: **nothing is resolved at runtime.** No registry, no reflection, no type
ids, no `get_codec`, no DTO, no mapper. `schema.json` is a build artifact and is
never read by a program that ships. The IR exists for the length of one
`cyclonec` process.

### 1.3 A timestamp in the header contradicts a byte-stable output

The brief requires `generated-at:` in every generated header. `cyclonec_old`
deliberately had no timestamp so that `--check` could compare file contents, and
so that regenerating did not touch mtimes and trigger rebuilds.

Both are satisfiable at once, and both are kept: the timestamp is written, and
the `// generated-at:` line is **excluded from every comparison**. A file whose
only difference is its timestamp is left alone, keeping the date of the run that
last actually changed it - which is the date a reader of that header wants
anyway. See `generator::same_but_for_timestamp`.

`.cyclone/schema.json` and `.cyclone/build-graph.json` carry **no timestamp at
all**: they are project contracts, they get committed and diffed, and a file
that changes every run cannot be either.

---

## 2. Deliberate changes

### 2.1 Rust, then Go, then C#, then GDScript, then C++, then C

The brief that started this rewrite asked for one target language at a time,
Rust first, and said explicitly not to add C#, Go, C++ or Godot. `parser/` and
`generator/` were shaped that way on purpose - so that a second language would
be a second module in each, with nothing above `ir.rs` moving - and Go, then
C#, then GDScript, then C++, then C, were added afterward, each by a direct,
explicit follow-up request that superseded that constraint for that language
specifically. Every language the original brief named out of scope has now
been added, and then one more besides (plain C, not named in the original
brief at all): none remain.

The prediction held five times over: `parser/go.rs`,
`generator/{go,go_runtime,go_handshake}.rs`, `parser/csharp.rs`,
`generator/{csharp,csharp_runtime,csharp_handshake}.rs`, `parser/gdscript.rs`,
`generator/{gdscript,gdscript_runtime,gdscript_handshake}.rs`, `parser/cpp.rs`,
`generator/{cpp,cpp_runtime,cpp_handshake}.rs`, `parser/c.rs` and
`generator/{c,c_runtime,c_handshake}.rs` are all new files, and `ir.rs`,
`fingerprint.rs`, `schema.rs`, `compat.rs` and `buildgraph.rs` did not change
for any of them. What *did* need a language-specific decision, because each
host's own compilation model forced one where Rust's didn't:

**Go:**

- **No module root.** Go compiles by package, not by file, so there is nothing
  for a `mod.rs` to declare - every file in one run shares a `package` clause,
  derived from `--out`'s own directory name, and a codec is reached the
  ordinary way with no root to mount.
- **Import paths come from `go.mod`, not from the source layout.** Rust's
  `crate::` is deducible purely from where `--src` points, because a Cargo
  crate root is always `src/`. Go has no such fixed point - an import path
  depends on the *module's* root, wherever it is - so the Go backend reads it
  from the nearest `go.mod`'s `module` line and requires that file to sit at
  the project root (`--model-path` overrides it, as `crate::models` does for
  Rust).
- **`Array<Array<T>>` is refused, not generated wrong.** `cyclonec_old`'s Go
  backend never handled a nested array correctly either - it emitted a call to
  a codec named after the literal `Array<...>` spelling, which cannot exist.
  This backend reports the same gap instead of reproducing the bug.

See [`generator/go.rs`](src/generator/go.rs) for the full reasoning.

**C#:**

- **No `import`, and so no import block at all.** Unlike Go, a fully-qualified
  C# reference (`Models.Player`) compiles without a `using` directive, so
  `generator::csharp` never has to compute or write one - it always spells a
  cross-namespace reference out in full instead, which is simpler than either
  the Rust or the Go backend's import bookkeeping.
- **Namespaces come from the source's own `namespace`, not a project file.**
  Unlike Go, C# needs no external file (`go.mod`) to compute this from: a
  model's namespace is read straight off its own source, the same directive
  comment [`parser::go::package_name`] reads a `package` clause with
  (`parser::csharp::namespace_name`), and "no namespace at all" - C#'s global
  namespace - is a valid, representable answer, not an error.
- **A local, not a borrow, for a nested model.** A C# property is not an
  addressable storage location and cannot be passed `ref` directly, unlike a
  Rust or Go struct field. A nested model field is decoded through a local:
  read the existing value out, decode into it by `ref`, assign it back -
  preserving the same "leaves fields this codec does not carry alone"
  guarantee the other two backends get from the language itself.
- **`Array<Array<T>>` is refused, not generated wrong** - the same choice as
  Go's, for the same reason: the element-type table this backend's whole
  knowledge of C# types lives in has no entry for `Array<T>` itself.

See [`generator/csharp.rs`](src/generator/csharp.rs) for the full reasoning.

**GDScript:**

- **One `class_name` reaches everything - no namespace, no import, no
  qualification at all.** `cyclonec_old`'s GDScript backend wrote one shared
  file for the whole project, wrapping every runtime type and codec inside
  one fixed `class_name`, specifically because a `.gd` file gets exactly one
  globally reachable name. This project's "one file per model per codec"
  layout turns out to fit that constraint *better*: every codec file declares
  its own `class_name`, Godot makes it reachable project-wide with nothing to
  `preload`, and unlike Go and C# there is no `Imports` concept in this
  backend at all - a model reference is always bare. `--model-path` has no
  effect on GDScript, because there is nothing left for it to override.
- **`static func`, now that a codec is its own file.** `cyclonec_old`
  instantiated every codec with `.new()` rather than risk an unresolved
  question about whether `static func` is well-formed *inside a nested
  class* - never shown as a combined example, and not checkable without a
  real Godot binary this project's tests cannot run. `encode`/`decode` here
  are declared at a codec file's own top level, where a GDScript `static
  func` is unambiguously, ordinarily valid, so that uncertainty never comes
  up.
- **RFC-0002 §9.1 needed a runtime method `cyclonec_old` never had.**
  `Reader.field_absent()` is new, the identical fix Rust, Go and C# each
  needed: the old runtime returned `unexpected_eof` for every read past the
  end, with no way to tell "this field never arrived" from "this field
  arrived truncated".
- **A signed 64-bit `int`, and no `u64` to hold a fingerprint in.**
  `cyclonec_old`'s GDScript backend never generated a 64-bit constant at all,
  so this question never came up for it. Every fingerprint constant here is
  built from two 32-bit halves, shifted and combined, rather than trusting an
  untested literal parser with a bare 16-digit hex value that might set the
  sign bit.
- **A handshake generator that did not exist before.** `cyclonec_old` had no
  GDScript handshake output whatsoever; `generator/gdscript_handshake.rs` is
  new, not a port, following C#'s shape (everything through one wrapper
  `class_name`) more than Go's (package-level constants).
- **`Array<Array<T>>` is refused, not generated wrong** - the same choice as
  Go's and C#'s, for the same reason: the element-type table this backend's
  whole knowledge of GDScript types lives in has no entry for `Array<T>`
  itself.

See [`generator/gdscript.rs`](src/generator/gdscript.rs) for the full
reasoning.

**C++:**

`cyclonec_old` never had a C++ backend at all - there is nothing here to port
against, only a brand-new header-only design, following this project's "one
file per model per codec" layout more closely than any of the ports did.

- **Macros, not attributes and not a comment directive.** C++ has no
  attribute syntax this generator can extend, and - unlike Go and GDScript -
  no established comment-directive convention to fall back on either. Instead
  a small header (shipped with the brief, not part of `cyclonec` itself)
  defines `CYCLONE_MODEL`, `CYCLONE_CODEC(...)` and `CYCLONE_FIELD(TYPE)` as
  macros that expand to nothing, the same "a few lines your models depend on,
  never the generated code" role `cyclone-attributes` plays for Rust and the
  `Network`/`Codec` pair plays for C#. `parser::cpp` reads them as ordinary
  tokens - not inside a `[...]` section the way C#'s `[Network]` is - since a
  real macro invocation has to look exactly like one.
- **A physical `#include`, because C++ has nothing else.** Go's `import` and
  C#'s bare fully-qualified name both work off a name already known to the
  compiler; C++ needs the actual header file. `generator::cpp::ModelLocation`
  carries the model's own source path for exactly this, and it is the one
  thing `--model-path` cannot override - only the *namespace* a model is
  qualified with.
- **Header-only, so nothing needed a module root.** Every method a generated
  `struct` declares is defined inside the struct body - implicitly `inline`,
  safe to `#include` from many translation units, with no `.cpp` half and
  nothing for a `mod.rs`-equivalent to declare, closer to GDScript's "no
  wrapper" shape than to Rust's module tree.
- **`DecodeError` returned, not thrown, and no C#-style local round-trip.** A
  generated model's fields are plain public members, so a nested model field
  is always addressable and passed by reference directly (no property
  workaround needed); and rather than following C#'s exceptions, every read
  takes its result by output reference and returns a `DecodeError` a project
  built with exceptions off can still call - the RFC-0002 §9.1 policy is the
  same `field_absent()` check every other backend uses.
- **No 64-bit workaround needed, unlike GDScript.** `std::uint64_t` is exact
  and unsigned, and the language itself promotes a hex literal too wide for
  `int`/`long` to `unsigned long long` - so a fingerprint is a plain
  `0x…ULL` literal.
- **`Array<Array<T>>` is refused, not generated wrong** - the same choice as
  every other backend but Rust's, for the same reason.

See [`generator/cpp.rs`](src/generator/cpp.rs) for the full reasoning.

**C:**

`cyclonec_old` never had a C backend either - not named in the original
brief's scope at all, added afterward on direct request, based on the same
header-only design C++'s backend already established, reshaped for a
language with no classes, no namespaces, no references, no exceptions and no
growable container types of its own. It also reassigned the `.h` extension:
C++'s backend originally recognised `.hpp`/`.h`/`.cpp`/`.cc`/`.cxx`, but `.h`
is ambiguous between the two languages, and a C project's models live in
headers as often as not - so `.h` now belongs to C alone, and a C++ project's
own headers are expected to use `.hpp`.

- **Macros, not attributes - the exact same three, from the exact same
  header C++ reads.** No new annotation convention was invented; `parser::c`
  is `parser::cpp`'s scanner minus everything C has no syntax for at all (no
  `class`, no `namespace`, no access specifiers, no templates, no raw
  strings) - smaller than C++'s scanner, not a restriction of it.
- **Free functions, not static methods.** C has nothing to hang a method off
  of, so where C++ writes `struct PlayerEdgeCodec { static bool encode(...);
  };`, C writes two `static inline` free functions,
  `PlayerEdgeCodec_encode`/`PlayerEdgeCodec_decode` - the same codec "type
  name" every backend already computes becomes a function-name prefix here
  instead of a `struct`'s own name.
- **`struct Name`, always - never bare `Name`.** The brief's own `DeviceState`
  example declares a plain tagged `struct DeviceState { ... };`, no
  `typedef` - so bare `DeviceState` is not a type in C at all. Every
  generated reference spells it `struct DeviceState`, which compiles whether
  or not a project also chooses to `typedef` it.
- **No namespace, so `generator::c::Imports` is nothing more than an
  `#include` lookup.** Unlike C++, there is no qualification step at all -
  `--model-path` has no effect on this backend, the same as GDScript, for a
  different reason (no namespace to begin with, rather than nothing left to
  override).
- **`bool` for a fallible write, because `malloc` can fail and `std::vector`
  cannot report that the way this backend needed.** Every generated `_encode`
  and every `CycloneWriter` method returns `bool`; decode keeps C++'s
  `CycloneDecodeError`-by-value shape.
- **`Array<T>` needed a type of its own - `arrays.h`.** C has no
  `std::vector<T>`, so an `Array<T>` field cannot be a raw `T*` plus a count
  without inventing a two-member field convention no other backend needs.
  Instead, one small owned `CycloneArray_T { items, count }` type is
  generated per *distinct* element type the schema uses, into a shared,
  schema-wide `arrays.h` - keeping the one-field-one-member invariant every
  backend's scanner already assumes.
- **`<Model>_free`, once per model, in its own file.** A `string`, `bytes` or
  `Array<T>` field - and every nested model field, recursively - is
  heap-owned once decoded, with nothing like `~Player()` to release it
  automatically. `<model>_cyclone.h` carries a single `<Model>_free` that
  walks every field any of that model's codecs ever decodes.
- **A real, once-hit circular-`#include` hazard, found and fixed by actually
  compiling the generated tree.** A first version of `arrays.h` also defined
  `CycloneArray_PlayerInfo_free`'s *body* there, which needed `struct
  PlayerInfo` complete to index `items[i]` - but `arrays.h` is a schema-wide
  file a model's own header may itself `#include` (to see the array types
  its fields need), so a model that both declares a type used as an array
  element *and* itself needs `arrays.h` creates a genuine `#include` cycle.
  The fix: a model element type's free loop is generated inline at each call
  site (which already has, or is given, the complete type through its own
  ordinary includes) instead of centralized in `arrays.h`, which now only
  ever needs a pointer to a model, never its complete shape. See
  `generator::c::free_array_value`'s doc comment.
- **`Array<Array<T>>` is refused, not generated wrong** - the same choice as
  every other backend but Rust's, for the same reason.

Unlike every backend before it, this one was validated against a real
toolchain in this sandbox (gcc) rather than only unit-tested: the committed
`tests/fixtures-c/` tree is actually compiled and its hand-written smoke test
actually run, in CI, the same as C++'s.

See [`generator/c.rs`](src/generator/c.rs) for the full reasoning.

### 2.2 One file per model per codec, as a module tree

| | old | new |
|---|---|---|
| output | `src/cyclone.codec.rs` - one file, whole project | `src/generated/player_edge.rs` - one per model per codec |
| runtime | inside that same file | `src/generated/runtime.rs`, once |
| reached by | `include!("cyclone.codec.rs")` | `mod generated;` |

The old output was one file **pasted into the user's own module**, which is why
nothing in it needed an import: the models were already in scope at the paste
site. A tree of files cannot work that way, and trying to keep it that way
produced two bugs worth naming, because both made the output unusable:

- **`player.edge.rs` is not a module name.** A dot is not part of a Rust
  identifier, so `mod player.edge;` does not parse and there is no other way to
  reach the file. Files are now named `player_edge.rs`.
- **A generated file named `Player` without importing it.** Fine inside an
  `include!`; a compile error in a module of its own.

So the tree is now an ordinary Rust module tree. `mod.rs` declares every module
and re-exports the runtime and every codec; each codec file imports what it
names:

```rust
use super::runtime::{DecodeError, Reader, Writer};
use crate::models::player::Player;
use super::player_info_edge::PlayerInfoEdgeCodec;
```

```rust
mod generated;
use generated::{PlayerEdgeCodec, Reader, Writer};
```

Still no crate and nothing to add to `Cargo.toml` - the tree is self-contained
exactly as the single file was.

### 2.3 The generator has to be told where your models live

That is the one thing an import needs and an `include!` did not. The default
reads it off the source layout, exactly as Rust does:

```text
src/models/player.rs   →  crate::models::player::Player
src/lib.rs             →  crate::Player
src/models/mod.rs      →  crate::models::Player
```

A project whose modules do not mirror its directories, or one that re-exports
every model from a single place, says so once:

```toml
model_path = "crate::models"
```

or `--model-path crate::models`. Models must also be `pub` and their annotated
fields visible to the generated module, which they had no need to be when the
codec was pasted in beside them.

Every generated file also opens with `#![allow(dead_code, unused_imports)]`. A
project uses the codecs it needs and no more, and `mod generated;` is private by
default - so without it, every unused constant is a warning in a file whose own
header says DO NOT EDIT.

### 2.4 The command line

```text
old:  cyclonec --out src/ src/
new:  cyclonec generate --src src --out generated

old:  cyclonec --check --out src/ src/
new:  cyclonec generate --check

new:  cyclonec compat --base .cyclone/schema.json
new:  cyclonec ci --base-ref origin/develop
```

- `--src` is repeatable and defaults to `cyclone.toml`'s `src`, then to `src`.
- `--out` defaults to `cyclone.toml`'s `out`, then to `generated`.
- A CLI flag always beats `cyclone.toml`.
- **`--stdout` is gone.** It meant something when the output was one file; a
  dozen files concatenated to a terminal is not a thing anybody wanted.
- There is still deliberately **no `--codec` flag**: a model declares its codecs
  in the source, and asking again on the command line could only ever disagree.

### 2.5 New errors, all of them at generate time

The old generator passed some malformed input through to the host compiler. What
it can now say precisely, it says:

| Input | Old | New |
|---|---|---|
| `#[network(Array<>)]` | emitted, `rustc`'s problem | `Array<>` has no element type |
| `#[network(Vec<u32>)]` | emitted as a model name | `Vec<u32>` is not a Cyclone type |
| two models with one name | both generated | declared twice, with both files |
| one model, two fields named the same | both generated | declared twice |
| two messages whose ids collide | n/a | reported, with both names |
| two constants spelled the same | n/a | reported, with both names |
| two codecs wanting the same module file | n/a | reported, with the file name |
| one message, two fields with one canonical name and one type (`ID: u32` and `Id: u32`) | n/a | reported, with both names |

Unchanged: a field whose Cyclone type names a model this run never parsed is
still left to the host compiler to resolve, and a nested field routed into a
codec the referenced model does not declare is still the one cross-model check
that runs before anything is written.

### 2.6 Field names are fingerprinted canonically (`cyclone-fingerprint/2`)

`cyclonec` reads one language per run, so a project with a Rust server and a Go
client has two annotated sources, each written the way its own language is
written. Under `cyclone-fingerprint/1` the field name was hashed exactly as the
source spelled it, so Rust's `id`, Go's `ID` and C#'s `Id` were three different
fingerprints for one wire contract - and the handshake said `REJECT` about peers
whose bytes were identical.

`/2` hashes the name with `_`, `-` and spaces removed and `A-Z` folded to `a-z`
(SPEC-FINGERPRINT.md §3.2), so all three read `field 0 id u32` and produce one
fingerprint. A rename a human meant - `x` to `position_x` - still changes the
fingerprint; only the convention stops counting.

**Every fingerprint in existence changed with this.** A `/1` peer and a `/2`
peer reject each other, which is what the version tag inside the hash is for.
Regenerate every language of a project together, and deploy them together:

```text
cyclonec generate     # in each language's own project root
```

Nothing about your models changes and nothing about your bytes changes - the
generated `encode`/`decode` still read `value.x` in Rust and `value.X` in C#.
The one new error is two fields of a single message sharing both a canonical
name and a type - `ID: u32` beside `Id: u32`, legal in Go and in C#, and the one
arrangement a fingerprint could not tell apart. Sharing a name but not a type,
or a name and a type but not a message, is not an error: those cannot hide a
reorder.

### 2.7 Still no dependencies

Not in the generator, and not in what it generates. SHA-256, JSON and the slice
of TOML that `cyclone.toml` needs are written out in this crate: a dependency is
a thing with a version, and the fingerprint algorithm must not have one.

The one exception is the same one `cyclonec_old` had - a **dev**-dependency on
`cyclone-attributes`, so that the annotated fixture compiles. That is not
incidental: `tests/fixtures/` is a real project, with one model annotated in
place and the generated codecs compiled against *that* type. A fixture that kept
a second, plain copy of every model to compile against would be demonstrating a
workflow nobody has.

---

## 3. What did not change

- The attribute syntax: `#[network]`, `#[codec(...)]`, `#[network(TYPE)]`.
- The scanner. It is the same lexer, the same struct walk, and the same single
  parse error - now carrying the line and file into the IR as well.
- The wire format, byte for byte. RFC-0002's runtime block is carried verbatim,
  and the only edit to it is the addition of `Reader::field_absent`.
- The wire type is never inferred from the host type. `#[network(u32)]` on a
  `u64` field is four bytes.
- Codec naming: `Player` + `orange_pi` → `PlayerOrangePiCodec`.
- A codec decodes only the fields it carries and leaves the rest as they were.
- A declared codec with no fields is still generated.

---

## 4. Migrating a project

1. Move your `--out` path to a directory inside your crate:
   `cyclonec generate --src src --out src/generated`.
2. Replace your `include!` with `mod generated;`, and `use generated::…` what
   you name. Set `model_path` if the default does not find your models.
3. Commit `.cyclone/schema.json`. It is the baseline every future comparison is
   made against, and CI cannot work without it.
4. Add the schema gate to CI - see [`.github/workflows/schema.yml`](.github/workflows/schema.yml).
   Point `--base-ref` at `github.base_ref`, never at a hard-coded branch.
5. Optionally add `cyclone.toml` so the flags stop being typed:

   ```toml
   src = "src"
   out = "generated"
   ```

Nothing about your models changes, and nothing about your peers' bytes changes.
