// The Go fixture the generator is tested against - the same schema as
// device_state.rs and device_state.cs, spelled in Go's directive-and-tag
// syntax, to prove all three produce the same codec names, the same field
// routing, and the same bytes.

package fixtures

//cyclone:model codec=edge,unity
type DeviceState struct {
	ID          uint32  `cyclone:"u32" codec:"edge,unity"`
	Temperature float32 `cyclone:"f32" codec:"edge"`
	Name        string  `cyclone:"string" codec:"unity"`

	// A network field in no codec: it is written by none of them.
	Unrouted uint32 `cyclone:"u32"`

	// Not a network field at all. Logic and caches stay off the wire.
	Cache string
}

//cyclone:model codec=edge,orange_pi,unity,custom_a
type Telemetry struct {
	Sequence uint64 `cyclone:"u64" codec:"edge,orange_pi,unity,custom_a"`
}

// A field whose network type is another model.
//
//cyclone:model codec=edge
type PlayerInfo struct {
	Level uint32 `cyclone:"u32" codec:"edge"`
}

//cyclone:model codec=edge
type Player struct {
	HP    uint32     `cyclone:"u32" codec:"edge"`
	Speed float32    `cyclone:"f32" codec:"edge"`
	Info  PlayerInfo `cyclone:"PlayerInfo" codec:"edge"`
}

// Array<T> - scalars, strings, and a nested model.
//
//cyclone:model codec=edge
type Team struct {
	Scores  []uint32     `cyclone:"Array<u32>" codec:"edge"`
	Names   []string     `cyclone:"Array<string>" codec:"edge"`
	Players []PlayerInfo `cyclone:"Array<PlayerInfo>" codec:"edge"`
}

// Every primitive RFC-0002 defines, once.
//
//cyclone:model codec=all
type EveryPrimitive struct {
	Flag bool    `cyclone:"bool" codec:"all"`
	A    int8    `cyclone:"i8" codec:"all"`
	B    uint8   `cyclone:"u8" codec:"all"`
	C    int16   `cyclone:"i16" codec:"all"`
	D    uint16  `cyclone:"u16" codec:"all"`
	E    int32   `cyclone:"i32" codec:"all"`
	F    uint32  `cyclone:"u32" codec:"all"`
	G    int64   `cyclone:"i64" codec:"all"`
	H    uint64  `cyclone:"u64" codec:"all"`
	I    float32 `cyclone:"f32" codec:"all"`
	J    float64 `cyclone:"f64" codec:"all"`
	K    string  `cyclone:"string" codec:"all"`
	L    []byte  `cyclone:"bytes" codec:"all"`
}

// A model that declares a codec no field joined.
//
//cyclone:model codec=lonely
type NoFieldsJoined struct {
	ID uint32 `cyclone:"u32"`
}

// //cyclone:model with no codec=: a model, nothing to generate.
//
//cyclone:model
type NoCodecs struct {
	ID uint32 `cyclone:"u32" codec:"edge"`
}

// Nothing marks this, so it is not a model.
type NotAModel struct {
	Whatever uint32
}

//cyclone:model codec=edge
type AfterTheUnmarkedStruct struct {
	Value uint32 `cyclone:"u32" codec:"edge"`
}
