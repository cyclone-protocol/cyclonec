# Cyclone Schema Fingerprint - `cyclone-fingerprint/2`

**Status:** normative for `cyclonec` 0.2 and every Cyclone SDK that interoperates with it.

A fingerprint identifies a **wire contract**: 32 bytes that two peers can compare
without sending a schema anywhere. It answers one question - *the same, or
different* - and nothing else. Working out *how* two schemas differ is a build
time job, done from two schemas, by a compatibility checker; a fingerprint is
never asked to explain itself.

One question two peers do have to answer at run time is narrower than "how":
*is the shorter field list an exact prefix of the longer one?* - the condition
RFC-0002 §9.1 makes a valid version difference. Section 3.5 covers it with a
fingerprint per prefix, which is still only ever compared, never explained.

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
cyclone-fingerprint/2
message <Model>.<codec>
field <index> <name> <type>
field <index> <name> <type>
...
end
```

- `<index>` counts from `0`, in decimal, over the fields **this codec carries**,
  in declaration order. A field the codec does not carry is not present and does
  not consume an index.
- `<name>` is the field's **canonical name**, section 3.2 - not the spelling the
  source used.
- `<type>` is section 3.4.

### 3.2 A field name

A field's name is hashed in one spelling, so that a project whose Rust half
writes `player_id` and whose Go half writes `PlayerID` has one schema and not
two. `cyclonec` reads one language per run, so the two halves of such a project
are two annotated sources, each written the way its own language is written;
under `cyclone-fingerprint/1` they hashed differently and their peers rejected
each other over a naming convention.

```
canonical_name = the name with every '_', '-' and ' ' removed,
                 with A-Z folded to a-z,
                 and every other character carried through unchanged.
```

If nothing is left (`_`), the canonical name is the original name.

Only ASCII is folded. Go and C# both accept non-ASCII identifiers, and Unicode
case folding is locale-sensitive in ways two SDKs can disagree about, so those
characters pass through byte for byte.

**Word boundaries are deliberately not recovered.** A rule that turned
`HTTPServer` into `http_server` must also turn `UserIDs` into `user_i_ds`, since
the two are the same shape - an uppercase run followed by lowercase - and Go's
`UserIDs` would then stop matching Rust's `user_ids`. Removing the separators
instead is unambiguous: every implementation reaches the same string without
having to agree where a word begins.

The price is that two names differing only in separator placement - `notify_url`
and `notif_yurl` - canonicalise together. Where that could hide a reorder it is
a generator error (section 3.2.1); everywhere else it is two spellings of one
field, at one offset, with one type - and the wire never carried the name.

#### 3.2.1 Collisions

**Two fields of one message MUST NOT share both a canonical name and a type
spelling.** `ID: u32` beside `Id: u32` is legal Go and legal C#, and both render
as `field <index> id u32`; swapping the two would leave the canonical text - and
so the fingerprint - unchanged, which is the one thing hashing names exists to
prevent (section 5). A generator MUST refuse such a schema rather than hash over
it.

The rule is exactly this wide and no wider. Two fields sharing a canonical name
but not a type are safe, because a swap moves the type spellings with them and
the text changes. Two fields sharing both, in codecs that never render them into
the same message, are safe for the same reason a fingerprint is per-message:
they never appear in one canonical text. A field in no codec appears in no
message at all.

The scope is therefore one message. A generator that also enforces the rule over
a model's whole field list is rejecting schemas no handshake can be fooled by.

#### 3.2.2 Test vectors

| Source spelling | Canonical name |
|---|---|
| `id`, `ID`, `Id` | `id` |
| `player_id`, `playerId`, `PlayerId`, `PlayerID`, `PLAYER_ID`, `player-id`, `__player_id__` | `playerid` |
| `http_server`, `HttpServer`, `HTTPServer` | `httpserver` |
| `user_ids`, `UserIds`, `UserIDs` | `userids` |
| `vec3`, `Vec3`, `vec_3` | `vec3` |
| `position3D`, `position_3d` | `position3d` |
| `café_id` | `caféid` |
| `_`, `__` | `_`, `__` |

### 3.3 A model

The fingerprint of a model's whole declaration - every annotated field, whatever
codec it joined. Not a wire contract; useful for "did `Player` change?".

```
cyclone-fingerprint/2
model <Model>
field <index> <name> <type>
...
end
```

### 3.4 A type

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
message, and the *model* body (section 3.3) when fingerprinting a model. The
body includes its own `\n` characters; it is hash input, not a format anything
parses back.

```
cyclone-fingerprint/2
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

### 3.5 A prefix

A message with `n` fields has `n` **prefix fingerprints**. The `k`th one, for
`k = 1..n`, is the fingerprint of **that same message truncated to its first
`k` fields**.

```
cyclone-fingerprint/2          cyclone-fingerprint/2
message Player.edge            message Player.edge
field 0 id u32          k=1    field 0 id u32          k=2
end                            field 1 x f32
                               end
```

There is no new rule here, and deliberately so. `<index>` already counts from
`0` over the fields this codec carries (section 3.1), and a nested model is
inlined into the field that holds it (section 3.4) - so dropping the tail
leaves every earlier field's text byte-for-byte unchanged. Truncation is just
section 3.1 applied to a shorter field list.

Two consequences follow, and both are load-bearing:

- **The `n`th prefix *is* the message fingerprint.** Nothing in section 3.1
  changes, so no digest that exists today moves. This is an addition, not a
  new canonical form, and the version tag stays `cyclone-fingerprint/2`.
- **The `k`th prefix of an `n`-field message equals the real fingerprint of a
  peer that declares only those `k` fields.** That is not a coincidence to be
  worked around; it is the entire mechanism.

**Why they exist.** A fingerprint answers "the same, or different" (section 1),
but RFC-0002 §9.1 asks a different question: *is the shorter field list an exact
prefix of the longer one?* Peers on either side of an appended field MUST
interoperate - RFC-0003 §8.6 pins that as vectors V-001/V-002 - and one digest
per message cannot tell "a field was appended" from "two same-typed fields were
swapped". The chain restores exactly the missing information and nothing else:
comparing at `k = min(n_local, n_peer)` is RFC-0002 §9.1's condition, expressed
in eight bytes.

**They are not a wire format.** The chain stays in each peer's own generated
code. A peer sends its field count and its last entry; the side doing the
comparison reads its own chain at the shared index. Only when the peer has more
fields than the local schema does the value live at an index the peer alone can
produce, and only then is one extra exchange needed.

**Computing them.** The obvious reading - hash each truncation independently -
is correct and is what the definition says. It is also `O(n²)` over the text. An
implementation may instead keep one running hash over the canonical text and,
after appending each field's line, finalise a *clone* of it with `end\n`
appended. Both produce identical bytes; the clone form is `O(n)`. Either is
fine, since this runs at build time.

### 3.6 A schema

Every message, by name, with its own digest, **sorted by the message name** as a
byte-wise ascending string comparison - so the order files happened to be
discovered in cannot change the answer.

```
cyclone-fingerprint/2
schema
message <name> <lowercase hex digest>
message <name> <lowercase hex digest>
...
end
```

### 3.7 The version tag

`cyclone-fingerprint/2` is inside the hash on purpose. A future canonical form
is `cyclone-fingerprint/3`, and old and new fingerprints then cannot silently
compare equal. **Changing the canonical form changes every fingerprint in
existence** and every deployed peer stops recognising every other one; it is a
coordinated, versioned, announced act, never a refactor.

| Tag | Canonical form |
|---|---|
| `/1` | Field names hashed exactly as the source spelled them. |
| `/2` | Field names hashed canonically (section 3.2), so that a naming convention is not a schema difference. |

A `/1` peer and a `/2` peer disagree about every fingerprint they hold, so the
handshake reports `REJECT` between them. That is the intended outcome and the
reason the tag is hashed: the two are not the same protocol, and no part of
either is safe to reuse against the other.

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
from the canonical form is not implementing `cyclone-fingerprint/2`.

`/2` narrows that price to the renames a human meant. Under `/1` the name was
hashed as written, so `id` in Rust, `ID` in Go and `Id` in C# were three
schemas - and since `cyclonec` reads one language per run, a project with more
than one language paid the false positive on every field it declared. Section
3.2 removes that case without touching the property this section is about: two
same-typed fields swapped still canonicalise to two different names in two
different positions, and still mismatch.

If a project decides it wants the other trade-off - no names hashed at all - it
is a new canonical form (`cyclone-fingerprint/3`) and a coordinated change
across every SDK, not a per-implementation option.

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
| a field recased or re-underscored (`Id` to `id`, `PlayerID` to `player_id`) | no - see section 3.2 |
| a nested model changed, anywhere inside | yes |
| a field added to a codec it did not join | yes, for that codec's message only |
| a comment, a host-language type, a file moved | no |

A fingerprint says only *different*. `cyclonec compat` and `cyclonec ci` say
which of these it was, from the two schemas, at build time.

The one distinction a peer can draw at run time is the prefix one. Comparing
prefix fingerprints (section 3.5) at `k = min(n_local, n_peer)` separates the
first row of that table - a field appended, which RFC-0002 §9.1 requires peers
to tolerate - from every other row, which moves a field at an index both ends
carry. That is the whole of what the handshake decides; it still cannot say
*which* of the other rows it was.

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
cyclone-fingerprint/2
message Player.edge
field 0 id u32
field 1 x f32
field 2 y f32
end
```

```
sha256:1c6d09808c8ba4ca9c7550a2fe664e114a2252a35161f95666644ec9b6eb7564
u64:    0x1C6D09808C8BA4CA
id:     0x432AB486
```

Its prefix chain (section 3.5), as 64-bit forms:

```
k=1   { id }           0x61B0FCFAB53A875E
k=2   { id, x }        0xF1ED8779E2A4A35D
k=3   { id, x, y }     0x1C6D09808C8BA4CA   <- the message fingerprint
```

A peer that really declares `Player.edge` as just `{ id, x }` computes
`0xF1ED8779E2A4A35D` as its own message fingerprint. That it lands on this
message's `k=2` entry is the mechanism, not a coincidence.

The same three digests come out of the Go struct that spells those fields
`ID`, `X`, `Y` and the C# class that spells them `Id`, `X`, `Y`: section 3.2 is
applied before the text above is built, so all three read `field 0 id u32`.

More pinned values - every message of a schema exercising nested models, arrays
and all thirteen primitives - are in
[`tests/vectors/cyclone-vectors.json`](tests/vectors/cyclone-vectors.json),
which is the artifact another SDK should check itself against.

---

## 8. Implementing this in another language

1. Build the ordered field list per message. Declaration order, always
   (RFC-0002 §5.1) - never reflection order, never memory order.
2. Canonicalise each field name (section 3.2) and refuse the schema if two of
   one message's fields collide (section 3.2.1). Check your canonicaliser
   against section 3.2.2 first - it is the cheapest thing to get subtly wrong,
   and it is the reason `/2` exists.
3. Render the canonical text exactly as section 3 says.
4. SHA-256 it.
5. Emit the prefix chain too (section 3.5): the same text truncated to the
   first `k` fields, for every `k`. Assert that the last entry equals the
   message fingerprint - if it does not, your truncation is not section 3.1
   applied to a shorter field list.
6. Check yourself against `tests/vectors/cyclone-vectors.json`. If one digest
   differs, the text differs; print your canonical text and diff it against the
   `canonical_example` in that file.

The reference implementation is
[`src/fingerprint.rs`](src/fingerprint.rs) - about a hundred lines, no
dependencies, and the same file the pinned vectors were produced by.
