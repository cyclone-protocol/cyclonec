# Codec names the generator has never heard of, and the PascalCase they turn
# into. No registration required - see h.md's item on codec targets: an
# unrecognized identifier is metadata, never a new Cyclone type.

# cyclone:model codec=edge,orange_pi,unity,custom_a
class_name Telemetry

# cyclone:u64 codec=edge,orange_pi,unity,custom_a
var sequence: int = 0
