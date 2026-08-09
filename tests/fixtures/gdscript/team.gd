# Array<T> - scalars, strings, and a nested model.

# cyclone:model codec=edge
class_name Team

# cyclone:Array<u32> codec=edge
var scores: Array[int] = []

# cyclone:Array<string> codec=edge
var names: Array[String] = []

# cyclone:Array<PlayerInfo> codec=edge
var players: Array[PlayerInfo] = []
