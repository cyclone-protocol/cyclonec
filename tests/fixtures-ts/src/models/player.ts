// The annotated source the TypeScript integration tests exercise - the
// TypeScript counterpart of tests/fixtures/src/models/player.rs: one model
// annotated in place, the generated codecs compiled against that same
// class, and the model RFC-0002 §9.1's version-skew tests decode.

// A field whose network type is another model.
// CYCLONE_MODEL
// CYCLONE_CODEC("edge")
export class PlayerInfo {
    // CYCLONE_FIELD(u32)
    // CYCLONE_CODEC("edge")
    Level: number = 0;
}

// The model RFC-0002 §9.1 is tested against: three fields, and a version
// that appends a fourth.
// CYCLONE_MODEL
// CYCLONE_CODEC("edge", "unity")
export class Player {
    // CYCLONE_FIELD(u32)
    // CYCLONE_CODEC("edge", "unity")
    Id: number = 0;

    // CYCLONE_FIELD(f32)
    // CYCLONE_CODEC("edge")
    X: number = 0;

    // CYCLONE_FIELD(f32)
    // CYCLONE_CODEC("edge")
    Y: number = 0;

    // A network field in no codec: it is written by none of them.
    // CYCLONE_FIELD(u32)
    Unrouted: number = 0;

    // Not a network field at all. Logic and caches stay off the wire.
    Cache: string = "";
}

// Composites: an array of primitives, an array of models, and a nested
// model.
// CYCLONE_MODEL
// CYCLONE_CODEC("edge")
export class Team {
    // CYCLONE_FIELD(PlayerInfo)
    // CYCLONE_CODEC("edge")
    Captain: PlayerInfo = new PlayerInfo();

    // CYCLONE_FIELD(Array<string>)
    // CYCLONE_CODEC("edge")
    Tags: string[] = [];

    // CYCLONE_FIELD(Array<u32>)
    // CYCLONE_CODEC("edge")
    Scores: number[] = [];

    // CYCLONE_FIELD(Array<PlayerInfo>)
    // CYCLONE_CODEC("edge")
    Roster: PlayerInfo[] = [];
}
