// The annotated source the JavaScript integration tests exercise - the
// JavaScript counterpart of tests/fixtures-ts/src/models/player.ts, with
// every type annotation dropped: the same annotations mean the same thing
// in both languages (issue.md §9).

// A field whose network type is another model.
// CYCLONE_MODEL
// CYCLONE_CODEC("edge")
export class PlayerInfo {
    // CYCLONE_FIELD(u32)
    // CYCLONE_CODEC("edge")
    Level = 0;
}

// The model RFC-0002 §9.1 is tested against: three fields, and a version
// that appends a fourth.
// CYCLONE_MODEL
// CYCLONE_CODEC("edge", "unity")
export class Player {
    // CYCLONE_FIELD(u32)
    // CYCLONE_CODEC("edge", "unity")
    Id = 0;

    // CYCLONE_FIELD(f32)
    // CYCLONE_CODEC("edge")
    X = 0;

    // CYCLONE_FIELD(f32)
    // CYCLONE_CODEC("edge")
    Y = 0;

    // A network field in no codec: it is written by none of them.
    // CYCLONE_FIELD(u32)
    Unrouted = 0;

    // Not a network field at all. Logic and caches stay off the wire.
    Cache = "";
}

// Composites: an array of primitives, an array of models, and a nested
// model.
// CYCLONE_MODEL
// CYCLONE_CODEC("edge")
export class Team {
    // CYCLONE_FIELD(PlayerInfo)
    // CYCLONE_CODEC("edge")
    Captain = new PlayerInfo();

    // CYCLONE_FIELD(Array<string>)
    // CYCLONE_CODEC("edge")
    Tags = [];

    // CYCLONE_FIELD(Array<u32>)
    // CYCLONE_CODEC("edge")
    Scores = [];

    // CYCLONE_FIELD(Array<PlayerInfo>)
    // CYCLONE_CODEC("edge")
    Roster = [];
}
