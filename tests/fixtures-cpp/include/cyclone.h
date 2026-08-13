#ifndef CYCLONE_H
#define CYCLONE_H

/*
 * Cyclone model/field/codec annotations, shared between C and C++.
 *
 * This header carries no dependency of its own (RFC-0001 §6.5) - it plays
 * the same role for C/C++ that the `cyclone-attributes` crate plays for
 * Rust and the small `Network`/`Codec` attribute pair plays for C#: a few
 * lines your models depend on, not `cyclonec` itself. `cyclonec` reads the
 * three markers below as source text; every one of them expands to nothing,
 * so an annotated model compiles unchanged on any C or C++ toolchain
 * whether or not `cyclonec` ever runs over it.
 *
 *   CYCLONE_MODEL             This struct is a Model (RFC-0002 §5). Placed
 *                              immediately above the struct.
 *
 *   CYCLONE_CODEC(names...)   Above the struct: the codecs generated for
 *                              this Model. Above a field: the codecs that
 *                              carry this field: a field with no
 *                              CYCLONE_CODEC of its own is declared but
 *                              written by none of them.
 *
 *   CYCLONE_FIELD(type)       This field's wire type (RFC-0002 §2.2), e.g.
 *                              bool, u8/i8 .. u64/i64, f32, f64, string,
 *                              bytes, Array<T>, or the name of another
 *                              Model. Placed immediately above the field.
 *
 * The wire type is never inferred from the host type - CYCLONE_FIELD(u32)
 * on a wider host field is still four bytes, Little Endian, on the wire.
 * Whether the host compiler accepts the field's declared type is the host
 * compiler's question, not this header's.
 */

#define CYCLONE_MODEL
#define CYCLONE_FIELD(type)
#define CYCLONE_CODEC(...)

#endif /* CYCLONE_H */
