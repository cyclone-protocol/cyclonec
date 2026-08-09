// The fixture the generator is tested against.
//
// `device_state.codec.rs` beside this file is what `cyclonec` wrote for it, and
// `tests/generated.rs` compiles both together and checks the calls it made.

use cyclone_attributes::*;

/// h.md §2 and §15 - two codecs, and a field in each combination of them.
#[network]
#[codec(edge, unity)]
#[derive(Debug, Default, PartialEq)]
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

    /// A network field in no codec: it is written by none of them.
    #[network(u32)]
    pub unrouted: u32,

    /// Not a network field at all. Logic and caches stay off the wire.
    pub cache: String,
}

/// h.md §16 - codec names the generator has never heard of, and the PascalCase
/// they turn into.
#[network]
#[codec(edge, orange_pi, unity, custom_a)]
#[derive(Debug, Default)]
pub struct Telemetry {
    #[network(u64)]
    #[codec(edge, orange_pi, unity, custom_a)]
    pub sequence: u64,
}

/// h.md §8 - a field whose network type is another model.
#[network]
#[codec(edge)]
#[derive(Debug, Default, PartialEq)]
pub struct PlayerInfo {
    #[network(u32)]
    #[codec(edge)]
    pub level: u32,
}

#[network]
#[codec(edge)]
#[derive(Debug, Default)]
pub struct Player {
    #[network(u32)]
    #[codec(edge)]
    pub hp: u32,

    #[network(f32)]
    #[codec(edge)]
    pub speed: f32,

    #[network(PlayerInfo)]
    #[codec(edge)]
    pub info: PlayerInfo,
}

/// Array<T> - scalars, strings, and a nested model, each as an element type.
#[network]
#[codec(edge)]
#[derive(Debug, Default, PartialEq)]
pub struct Team {
    #[network(Array<u32>)]
    #[codec(edge)]
    pub scores: Vec<u32>,

    #[network(Array<string>)]
    #[codec(edge)]
    pub names: Vec<String>,

    #[network(Array<PlayerInfo>)]
    #[codec(edge)]
    pub players: Vec<PlayerInfo>,
}

/// Every primitive RFC-0002 defines, once.
#[network]
#[codec(all)]
#[derive(Debug, Default)]
pub struct EveryPrimitive {
    #[network(bool)]
    #[codec(all)]
    pub flag: bool,

    #[network(i8)]
    #[codec(all)]
    pub a: i8,

    #[network(u8)]
    #[codec(all)]
    pub b: u8,

    #[network(i16)]
    #[codec(all)]
    pub c: i16,

    #[network(u16)]
    #[codec(all)]
    pub d: u16,

    #[network(i32)]
    #[codec(all)]
    pub e: i32,

    #[network(u32)]
    #[codec(all)]
    pub f: u32,

    #[network(i64)]
    #[codec(all)]
    pub g: i64,

    #[network(u64)]
    #[codec(all)]
    pub h: u64,

    #[network(f32)]
    #[codec(all)]
    pub i: f32,

    #[network(f64)]
    #[codec(all)]
    pub j: f64,

    #[network(string)]
    #[codec(all)]
    pub k: String,

    #[network(bytes)]
    #[codec(all)]
    pub l: Vec<u8>,
}

/// A model that declares a codec no field joined. §15 says a declared codec is
/// generated, so this one is generated empty rather than dropped.
#[network]
#[codec(lonely)]
#[derive(Debug, Default)]
pub struct NoFieldsJoined {
    #[network(u32)]
    pub id: u32,
}

/// `#[network]` with no `#[codec(...)]`: a model, but nothing to generate.
#[network]
#[derive(Debug, Default)]
pub struct NoCodecs {
    #[network(u32)]
    #[codec(edge)]
    pub id: u32,
}

/// Nothing marks this, so it is not a model - and its annotated-looking fields
/// must not leak into the model declared after it.
#[derive(Debug, Default)]
pub struct NotAModel {
    pub whatever: u32,
}

#[network]
#[codec(edge)]
#[derive(Debug, Default)]
pub struct AfterTheUnmarkedStruct {
    #[network(u32)]
    #[codec(edge)]
    pub value: u32,
}
