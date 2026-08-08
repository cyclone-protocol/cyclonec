# `# cyclone:model` with no `codec=` at all: a model, but nothing to generate -
# the same as bare `#[network]` in Rust, bare `[Network]` in C#, and
# `//cyclone:model` with no codec in Go.

# cyclone:model
class_name NoCodecs

# cyclone:u32 codec=edge
var id: int = 0
