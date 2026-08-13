# player.gd is the model RFC-0002 §9.1's version-skew tests decode: three
# fields, and a version that appends a fourth - the GDScript counterpart of
# the Player class in tests/fixtures-cs/src/models/Player.cs.
# cyclone:model codec=edge,unity
class_name Player

# cyclone:u32 codec=edge,unity
var id: int = 0

# cyclone:f32 codec=edge
var x: float = 0.0

# cyclone:f32 codec=edge
var y: float = 0.0

# A network field in no codec: it is written by none of them.
# cyclone:u32
var unrouted: int = 0

# Not a network field at all. Logic and caches stay off the wire.
var cache: String = ""
