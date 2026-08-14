// Every primitive RFC-0002 §2 defines, in one model, in one codec - the
// JavaScript counterpart of tests/fixtures-ts/src/models/every_primitive.ts.

// CYCLONE_MODEL
// CYCLONE_CODEC("edge")
export class EveryPrimitive {
    // CYCLONE_FIELD(bool)
    // CYCLONE_CODEC("edge")
    Flag = false;

    // CYCLONE_FIELD(i8)
    // CYCLONE_CODEC("edge")
    Tiny = 0;

    // CYCLONE_FIELD(u8)
    // CYCLONE_CODEC("edge")
    Byte = 0;

    // CYCLONE_FIELD(i16)
    // CYCLONE_CODEC("edge")
    Small = 0;

    // CYCLONE_FIELD(u16)
    // CYCLONE_CODEC("edge")
    Port = 0;

    // CYCLONE_FIELD(i32)
    // CYCLONE_CODEC("edge")
    Offset = 0;

    // CYCLONE_FIELD(u32)
    // CYCLONE_CODEC("edge")
    Count = 0;

    // CYCLONE_FIELD(i64)
    // CYCLONE_CODEC("edge")
    Delta = 0n;

    // CYCLONE_FIELD(u64)
    // CYCLONE_CODEC("edge")
    Sequence = 0n;

    // CYCLONE_FIELD(f32)
    // CYCLONE_CODEC("edge")
    Ratio = 0;

    // CYCLONE_FIELD(f64)
    // CYCLONE_CODEC("edge")
    Precise = 0;

    // CYCLONE_FIELD(string)
    // CYCLONE_CODEC("edge")
    Label = "";

    // CYCLONE_FIELD(bytes)
    // CYCLONE_CODEC("edge")
    Blob = new Uint8Array(0);
}
