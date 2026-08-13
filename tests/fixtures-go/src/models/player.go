// Package models is the annotated source the Go integration tests exercise -
// the Go counterpart of tests/fixtures/src/models/player.rs: one model
// annotated in place, the generated codecs compiled against that same type,
// and the model RFC-0002 §9.1's version-skew tests decode.
package models

// PlayerInfo is a field whose network type is another model.
//
//cyclone:model codec=edge
type PlayerInfo struct {
	Level uint32 `cyclone:"u32" codec:"edge"`
}

// Player is the model RFC-0002 §9.1 is tested against: three fields, and a
// version that appends a fourth.
//
//cyclone:model codec=edge,unity
type Player struct {
	ID uint32  `cyclone:"u32" codec:"edge,unity"`
	X  float32 `cyclone:"f32" codec:"edge"`
	Y  float32 `cyclone:"f32" codec:"edge"`

	// A network field in no codec: it is written by none of them.
	Unrouted uint32 `cyclone:"u32"`

	// Not a network field at all. Logic and caches stay off the wire.
	Cache string
}

// Team holds composites: an array of primitives, an array of models, and a
// nested model.
//
//cyclone:model codec=edge
type Team struct {
	Captain PlayerInfo   `cyclone:"PlayerInfo" codec:"edge"`
	Tags    []string     `cyclone:"Array<string>" codec:"edge"`
	Scores  []uint32     `cyclone:"Array<u32>" codec:"edge"`
	Roster  []PlayerInfo `cyclone:"Array<PlayerInfo>" codec:"edge"`
}
