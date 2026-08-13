use cyclone_attributes::*;

// A model a user writes, annotated in place. This is the type `cyclonec` scans
// *and* the type its generated codec encodes and decodes - there is no second
// copy of it, and none is generated.

/// Two codecs, and a field in each combination of them.
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

    /// A network field in no codec: written by none of them.
    #[network(u32)]
    pub unrouted: u32,

    /// Not a network field at all. Logic and caches stay off the wire.
    pub cache: String,
}

/// Codec names the generator has never heard of, and the PascalCase they turn
/// into.
#[network]
#[codec(edge, orange_pi)]
#[derive(Debug, Default)]
pub struct Telemetry {
    #[network(u64)]
    #[codec(edge, orange_pi)]
    pub sequence: u64,
}
