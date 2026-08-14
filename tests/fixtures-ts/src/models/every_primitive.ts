// Every primitive RFC-0002 §2 defines, in one model, in one codec - the
// TypeScript counterpart of tests/fixtures/src/models/every_primitive.rs.

// CYCLONE_MODEL
// CYCLONE_CODEC("edge")
export class EveryPrimitive {
    // CYCLONE_FIELD(bool)
    // CYCLONE_CODEC("edge")
    Flag: boolean = false;

    // CYCLONE_FIELD(i8)
    // CYCLONE_CODEC("edge")
    Tiny: number = 0;

    // CYCLONE_FIELD(u8)
    // CYCLONE_CODEC("edge")
    Byte: number = 0;

    // CYCLONE_FIELD(i16)
    // CYCLONE_CODEC("edge")
    Small: number = 0;

    // CYCLONE_FIELD(u16)
    // CYCLONE_CODEC("edge")
    Port: number = 0;

    // CYCLONE_FIELD(i32)
    // CYCLONE_CODEC("edge")
    Offset: number = 0;

    // CYCLONE_FIELD(u32)
    // CYCLONE_CODEC("edge")
    Count: number = 0;

    // CYCLONE_FIELD(i64)
    // CYCLONE_CODEC("edge")
    Delta: bigint = 0n;

    // CYCLONE_FIELD(u64)
    // CYCLONE_CODEC("edge")
    Sequence: bigint = 0n;

    // CYCLONE_FIELD(f32)
    // CYCLONE_CODEC("edge")
    Ratio: number = 0;

    // CYCLONE_FIELD(f64)
    // CYCLONE_CODEC("edge")
    Precise: number = 0;

    // CYCLONE_FIELD(string)
    // CYCLONE_CODEC("edge")
    Label: string = "";

    // CYCLONE_FIELD(bytes)
    // CYCLONE_CODEC("edge")
    Blob: Uint8Array = new Uint8Array(0);
}
