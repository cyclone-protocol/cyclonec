# The GDScript fixture the generator is tested against - the same schema as
# device_state.rs/.cs/.go, spelled in GDScript's comment-directive syntax, to
# prove all four produce the same codec names, the same field routing, and
# the same bytes.
#
# Split one model per file (unlike the other three fixtures): Godot allows
# exactly one `class_name` per script, so this is also the idiomatic way a
# real Godot project would lay these out.

# cyclone:model codec=edge,unity
class_name DeviceState

# cyclone:u32 codec=edge,unity
var id: int = 0

# cyclone:f32 codec=edge
var temperature: float = 0.0

# cyclone:string codec=unity
var display_name: String = ""

# A network field in no codec: it is written by none of them.
# cyclone:u32
var unrouted: int = 0

# Not a network field at all. Logic and caches stay off the wire.
var cache: String = ""
