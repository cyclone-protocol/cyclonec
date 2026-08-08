# A model that declares a codec no field joined. A declared codec is still
# generated - empty rather than dropped.

# cyclone:model codec=lonely
class_name NoFieldsJoined

# cyclone:u32
var id: int = 0
