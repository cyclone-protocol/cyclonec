# Cyclone Schema Fingerprint - `cyclone-fingerprint/1`

**Status:** normative for `cyclonec` 0.2 and every Cyclone SDK that interoperates with it.

A fingerprint identifies a **wire contract**: 32 bytes that two peers can compare
without sending a schema anywhere. It answers one question - *the same, or
different* - and nothing else. Working out *how* two schemas differ is a build
time job, done from two schemas, by a compatibility checker; a fingerprint is
never asked to explain itself.

This document defines the exact bytes that get hashed. It exists because
"deterministic" is not a property an implementation can have on its own: Rust,
Go, C#, C++ and every future SDK must produce **byte-identical digests from the
same schema**, forever, and that is only true if the input to the hash is
specified rather than chosen.

---

## 1. Definitions

| Term | Meaning |
|---|---|
| **model** | A type marked as a Cyclone network model, with an ordered list of fields. |
| **codec** | A named subset of a model's fields, in declaration order. |
| **message** | One model rendered by one codec. `Player` + `edge` is the message `Player.edge`. **A message is the thing that goes on the wire, and the thing a fingerprint identifies.** |
| **schema** | Every model of one project, and therefore every message. |

A model with three codecs is three messages and three fingerprints. Two codecs
of one model may evolve independently, and frequently do.

---

## 2. The digest

```
digest      = SHA-256(canonical_text encoded as UTF-8)
text form   = "sha256:" + lowercase hex of all 32 bytes
64-bit form = the first 8 bytes of the digest, big endian
```

The 64-bit form is what a generated constant and a handshake frame carry; it is
what fits in a register. The full digest stays in `schema.json` for anything
that needs to be certain.

SHA-256 is FIPS 180-4. Every SDK is expected to use its standard library's.

---

## 3. The canonical text

ASCII, `\n` line endings, one space between tokens, and a trailing newline.
No indentation, no alignment, no trailing spaces, no `\r`.

### 3.1 A message

```
cyclone-fingerprint/1
message <Model>.<codec>
field <index> <name> <type>
field <index> <name> <type>
...
end
```

- `<index>` counts from `0`, in decimal, over the fields **this codec carries**,
  in declaration order. A field the codec does not carry is not present and does
  not consume an index.
- `<name>` is the field name exactly as the source spells it.
- `<type>` is section 3.3.

### 3.2 A model

The fingerprint of a model's whole declaration - every annotated field, whatever
codec it joined. Not a wire contract; useful for "did `Player` change?".

```
cyclone-fingerprint/1
model <Model>
field <index> <name> <type>
...
end
```

### 3.3 A type

| Cyclone type | Canonical spelling |
|---|---|
| `bool` `i8` `u8` `i16` `u16` `i32` `u32` `i64` `u64` `f32` `f64` | itself |
| `string` | `string` |
| `bytes` | `bytes` |
| `Array<T>` | `Array<` + canonical spelling of `T` + `>` |
| a model this schema declares | `model<Name:` + that model's own body + `>` |
| a model this schema does not declare | `extern<Name>` |
| a model already being expanded | `recursive<Name>` |

**Nested models are expanded inline**, because that is what the bytes do
(RFC-0002 §5): a nested model contributes its fields to the stream at that
offset, so a change inside it moves everything after it. The body inserted is
the *message* body (section 3.1) under the same codec when fingerprinting a
message, and the *model* body (section 3.2) when fingerprinting a model. The
body includes its own `\n` characters; it is hash input, not a format anything
parses back.

```
cyclone-fingerprint/1
message Team.edge
field 0 captain model<PlayerInfo:message PlayerInfo.edge
field 0 level u32
end
>
field 1 tags Array<string>
end
```

**`extern<Name>`** is a type this run never parsed - hand-written, or from
another package. Its layout is not visible, so its name is all there is to hash.
A later schema that does declare it produces a different fingerprint, which is
correct: it is a different, now-known, contract.

**`recursive<Name>`** terminates expansion when a model is reached that is
already on the expansion stack (reachable through `Array<T>`), without losing
the fact that the recursion is there. The stack holds model names, is pushed
before a body is expanded and popped after.

### 3.4 A schema

Every message, by name, with its own digest, **sorted by the message name** as a
byte-wise ascending string comparison - so the order files happened to be
discovered in cannot change the answer.

```
cyclone-fingerprint/1
schema
message <name> <lowercase hex digest>
message <name> <lowercase hex digest>
...
end
```

### 3.5 The version tag

`cyclone-fingerprint/1` is inside the hash on purpose. A future canonical form
is `cyclone-fingerprint/2`, and old and new fingerprints then cannot silently
compare equal. **Changing the canonical form changes every fingerprint in
existence** and every deployed peer stops recognising every other one; it is a
coordinated, versioned, announced act, never a refactor.

---

## 4. Message ids

A message also has a 32-bit id, derived from its **name alone**:

```
message_id = first 4 bytes, big endian, of
             SHA-256("cyclone-message-id/1\n" + <Model>.<codec> + "\n")
```

Deliberately not derived from the fingerprint. An id names a message; a
fingerprint describes its current shape. An id that changed whenever a field was
appended could not be used to look a message up - which is exactly what a peer
needs to do while disagreeing about that message's shape.

Collisions are possible in principle and are a generator error, not a runtime
hazard: `cyclonec` refuses to generate a schema whose message ids collide.

---

## 5. Why field names are hashed

The wire format does not carry field names. Position is the only identifier
(RFC-0001 §4.1). Hashing only the types and their order would therefore be a
defensible reading of "a fingerprint represents wire layout".

It is also unsafe, and this is where the brief's two requirements meet:

> - a fingerprint must detect field **reorder**
> - if the wire format does not depend on field names, a fingerprint need not
>   hash them

Both hold, except in one case where they contradict each other:

```
v1: x: f32, y: f32          v2: y: f32, x: f32
```

Types-only, those two hash **identically** - same types, same offsets, same
byte count. Two peers would shake hands, agree they are current, and then
quietly transpose every coordinate they exchange for the lifetime of the
deployment. Hashing the names makes it a mismatch, which a handshake can act on.

The price is a false positive: a pure rename - `x` to `position_x`, not one byte
on the wire changed - also changes the fingerprint, and peers that could have
talked will refuse to. That failure is loud, immediate and harmless; the one it
replaces is silent, permanent and corrupts data. **The choice is deliberate and
recorded here so that no SDK "fixes" it independently.** An SDK that drops names
from the canonical form is not implementing `cyclone-fingerprint/1`.

If a project decides it wants the other trade-off, it is a new canonical form
(`cyclone-fingerprint/2`) and a coordinated change across every SDK - not a
per-implementation option.

---

## 6. What a fingerprint detects

Against the changes the brief enumerates, for a message:

| Change | Fingerprint changes |
|---|---|
| a field appended at the end | yes |
| a field inserted in the middle | yes |
| a field removed | yes |
| fields reordered (different types) | yes |
| fields reordered (same types) | yes - because names are hashed |
| a field's wire type changed | yes |
| a field renamed, nothing else | yes - see section 5 |
| a nested model changed, anywhere inside | yes |
| a field added to a codec it did not join | yes, for that codec's message only |
| a comment, a host-language type, a file moved | no |

A fingerprint says only *different*. `cyclonec compat` and `cyclonec ci` say
which of these it was, from the two schemas, at build time.

---

## 7. Worked example

Schema:

```rust
#[network]
#[codec(edge)]
struct Player {
    #[network(u32)] #[codec(edge)] id: u32,
    #[network(f32)] #[codec(edge)] x: f32,
    #[network(f32)] #[codec(edge)] y: f32,
}
```

Canonical text for `Player.edge` (`\n` shown as line breaks, and the file ends
with one):

```
cyclone-fingerprint/1
message Player.edge
field 0 id u32
field 1 x f32
field 2 y f32
end
```

```
sha256:231dd2d8744fecc3198c9853ffafe18023c93670fe7822c4cd9638fe9eabbe8b
u64:    0x231DD2D8744FECC3
id:     0x432AB486
```

More pinned values - every message of a schema exercising nested models, arrays
and all thirteen primitives - are in
[`tests/vectors/cyclone-vectors.json`](tests/vectors/cyclone-vectors.json),
which is the artifact another SDK should check itself against.

---

## 8. Implementing this in another language

1. Build the ordered field list per message. Declaration order, always
   (RFC-0002 §5.1) - never reflection order, never memory order.
2. Render the canonical text exactly as section 3 says.
3. SHA-256 it.
4. Check yourself against `tests/vectors/cyclone-vectors.json`. If one digest
   differs, the text differs; print your canonical text and diff it against the
   `canonical_example` in that file.

The reference implementation is
[`src/fingerprint.rs`](src/fingerprint.rs) - about a hundred lines, no
dependencies, and the same file the pinned vectors were produced by.
