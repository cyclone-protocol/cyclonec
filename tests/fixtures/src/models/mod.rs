// The models a user writes: annotated once, compiled once, encoded in place.
//
// `cyclonec` scans these files and writes `../generated/` from them; the
// generated codecs then `use crate::models::player::Player` - this very type.
// There is no second copy of a model anywhere, and none is generated.

pub mod device_state;
pub mod every_primitive;
pub mod player;
