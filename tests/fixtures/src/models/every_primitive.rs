use cyclone_attributes::*;

// Every primitive RFC-0002 §2 defines, in one model, in one codec - the model
// the fixed wire vectors are taken over.

#[network]
#[codec(edge)]
#[derive(Debug, Default, PartialEq)]
pub struct EveryPrimitive {
    #[network(bool)]
    #[codec(edge)]
    pub flag: bool,

    #[network(i8)]
    #[codec(edge)]
    pub tiny: i8,

    #[network(u8)]
    #[codec(edge)]
    pub byte: u8,

    #[network(i16)]
    #[codec(edge)]
    pub small: i16,

    #[network(u16)]
    #[codec(edge)]
    pub port: u16,

    #[network(i32)]
    #[codec(edge)]
    pub offset: i32,

    #[network(u32)]
    #[codec(edge)]
    pub count: u32,

    #[network(i64)]
    #[codec(edge)]
    pub delta: i64,

    #[network(u64)]
    #[codec(edge)]
    pub sequence: u64,

    #[network(f32)]
    #[codec(edge)]
    pub ratio: f32,

    #[network(f64)]
    #[codec(edge)]
    pub precise: f64,

    #[network(string)]
    #[codec(edge)]
    pub label: String,

    #[network(bytes)]
    #[codec(edge)]
    pub blob: Vec<u8>,
}
