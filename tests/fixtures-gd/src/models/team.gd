# team.gd holds composites: an array of primitives, an array of models, and a
# nested model - the GDScript counterpart of the Team class in
# tests/fixtures-cs/src/models/Player.cs.
# cyclone:model codec=edge
class_name Team

# cyclone:PlayerInfo codec=edge
var captain: PlayerInfo = PlayerInfo.new()

# cyclone:Array<string> codec=edge
var tags: Array = []

# cyclone:Array<u32> codec=edge
var scores: Array = []

# cyclone:Array<PlayerInfo> codec=edge
var roster: Array = []
