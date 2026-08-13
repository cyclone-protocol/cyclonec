use cyclone_attributes::*;

// Nested models and arrays, and the model the version-skew tests decode.

/// A field whose network type is another model.
#[network]
#[codec(edge)]
#[derive(Debug, Default, PartialEq)]
pub struct PlayerInfo {
    #[network(u32)]
    #[codec(edge)]
    pub level: u32,
}

/// The model RFC-0002 §9.1 is tested against: three fields, and a version that
/// appends a fourth.
#[network]
#[codec(edge)]
#[derive(Debug, Default, PartialEq)]
pub struct Player {
    #[network(u32)]
    #[codec(edge)]
    pub id: u32,

    #[network(f32)]
    #[codec(edge)]
    pub x: f32,

    #[network(f32)]
    #[codec(edge)]
    pub y: f32,
}

/// Composites: an array of primitives, an array of models, and a nested model.
#[network]
#[codec(edge)]
#[derive(Debug, Default, PartialEq)]
pub struct Team {
    #[network(PlayerInfo)]
    #[codec(edge)]
    pub captain: PlayerInfo,

    #[network(Array<string>)]
    #[codec(edge)]
    pub tags: Vec<String>,

    #[network(Array<u32>)]
    #[codec(edge)]
    pub scores: Vec<u32>,

    #[network(Array<PlayerInfo>)]
    #[codec(edge)]
    pub roster: Vec<PlayerInfo>,
}
