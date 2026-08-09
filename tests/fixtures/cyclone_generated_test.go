// End-to-end: the Go backend's output, compiled and run.
//
// A generator that emits plausible-looking source proves nothing. This file
// lives in the same package as device_state.go (the models) and
// cyclone.codec.go (what cyclonec generated from them), so every codec used
// below is code the Go compiler accepted.
//
// There is no stub and no import of anything but the standard library brought
// in by cyclone.codec.go itself. The bytes checked here are the bytes a user
// would put on the wire, compared against RFC-0002 - and every expectation is
// copied from tests/generated.rs (Rust) and tests/csharp/GeneratedTests.cs
// (C#), which is the whole of h.md §23's proof: the same schema, read from
// three different syntaxes, produces the same bytes.

package fixtures

import (
	"bytes"
	"math"
	"testing"
)

func sample() *DeviceState {
	return &DeviceState{
		ID:          42,
		Temperature: 21.5,
		Name:        "sensor-1",
		Unrouted:    7,
		Cache:       "local",
	}
}

func encodeWith(t *testing.T, value *DeviceState, encode func(*Writer, *DeviceState)) []byte {
	t.Helper()
	w := NewWriter()
	encode(w, value)
	return w.Bytes()
}

// ================================================== §9/§18 - one model, two codecs

// h.md §9 - EdgeCodec carries ID and Temperature, UnityCodec carries ID and
// Name, each in declaration order. Same bytes as the Rust and C# tests.
func TestEachCodecWritesTheFieldsThatNamedIt(t *testing.T) {
	edge := (DeviceStateEdgeCodec{})
	unity := (DeviceStateUnityCodec{})

	wantEdge := []byte{
		0x2A, 0x00, 0x00, 0x00, // ID = 42, u32 Little Endian
		0x00, 0x00, 0xAC, 0x41, // Temperature = 21.5, raw IEEE 754 bits
	}
	if got := encodeWith(t, sample(), edge.Encode); !bytes.Equal(got, wantEdge) {
		t.Errorf("edge encode = % X, want % X", got, wantEdge)
	}

	wantUnity := []byte{
		0x2A, 0x00, 0x00, 0x00, // ID = 42
		0x08, 0x00, 0x00, 0x00, // "sensor-1" - a length in bytes
		0x73, 0x65, 0x6E, 0x73, 0x6F, 0x72, 0x2D, 0x31,
	}
	if got := encodeWith(t, sample(), unity.Encode); !bytes.Equal(got, wantUnity) {
		t.Errorf("unity encode = % X, want % X", got, wantUnity)
	}
}

func TestEachCodecRoundTrips(t *testing.T) {
	edge := DeviceStateEdgeCodec{}
	data := encodeWith(t, sample(), edge.Encode)

	value := &DeviceState{}
	r := NewReader(data)
	if err := edge.Decode(r, value); err != nil {
		t.Fatalf("decode: %v", err)
	}

	if value.ID != 42 {
		t.Errorf("ID = %d, want 42", value.ID)
	}
	if value.Temperature != 21.5 {
		t.Errorf("Temperature = %v, want 21.5", value.Temperature)
	}
	if !r.IsEmpty() {
		t.Errorf("cursor did not land exactly at the end: %d bytes remaining", r.Remaining())
	}
}

// A codec leaves the fields it does not carry exactly as they were, which is
// what lets one model be split across several of them.
func TestDecodeLeavesFieldsItDoesNotCarryAlone(t *testing.T) {
	edge := DeviceStateEdgeCodec{}
	data := encodeWith(t, sample(), edge.Encode)

	value := sample()
	value.ID = 0
	value.Temperature = 0

	if err := edge.Decode(NewReader(data), value); err != nil {
		t.Fatalf("decode: %v", err)
	}

	if value.ID != 42 || value.Temperature != 21.5 {
		t.Errorf("edge fields were not restored: ID=%d Temperature=%v", value.ID, value.Temperature)
	}
	if value.Name != "sensor-1" || value.Unrouted != 7 || value.Cache != "local" {
		t.Errorf("fields edge does not carry were disturbed: %+v", value)
	}
}

// Both codecs applied in turn rebuild every routed field, and only those.
func TestTwoCodecsTogetherCoverTheRoutedFields(t *testing.T) {
	edge := DeviceStateEdgeCodec{}
	unity := DeviceStateUnityCodec{}

	edgeBytes := encodeWith(t, sample(), edge.Encode)
	unityBytes := encodeWith(t, sample(), unity.Encode)

	value := &DeviceState{}
	if err := edge.Decode(NewReader(edgeBytes), value); err != nil {
		t.Fatalf("edge decode: %v", err)
	}
	if err := unity.Decode(NewReader(unityBytes), value); err != nil {
		t.Fatalf("unity decode: %v", err)
	}

	if value.ID != 42 || value.Temperature != 21.5 || value.Name != "sensor-1" {
		t.Errorf("routed fields incomplete: %+v", value)
	}
	// A field in no codec, and a field with no cyclone tag, are on no wire.
	if value.Unrouted != 0 || value.Cache != "" {
		t.Errorf("unrouted fields were written by a codec that does not carry them: %+v", value)
	}
}

// ========================================================== §7 - codec names

// h.md §7 - every identifier is a codec name, and the four types exist.
func TestUnknownCodecNamesBecomeGeneratedTypes(t *testing.T) {
	value := &Telemetry{Sequence: 9}
	want := []byte{0x09, 0, 0, 0, 0, 0, 0, 0}

	codecs := []struct {
		name   string
		encode func(*Writer, *Telemetry)
	}{
		{"edge", (TelemetryEdgeCodec{}).Encode},
		{"orange_pi", (TelemetryOrangePiCodec{}).Encode},
		{"unity", (TelemetryUnityCodec{}).Encode},
		{"custom_a", (TelemetryCustomACodec{}).Encode},
	}

	for _, c := range codecs {
		w := NewWriter()
		c.encode(w, value)
		if got := w.Bytes(); !bytes.Equal(got, want) {
			t.Errorf("%s: encode = % X, want % X", c.name, got, want)
		}
	}
}

// ======================================================= §14 - composite model

// h.md §14 - a model-typed field becomes a call to that model's codec,
// inlined: no length, no delimiter, no header.
func TestAModelFieldIsInlined(t *testing.T) {
	edge := PlayerEdgeCodec{}
	value := &Player{HP: 100, Speed: 1.5, Info: PlayerInfo{Level: 3}}

	w := NewWriter()
	edge.Encode(w, value)
	got := w.Bytes()

	want := []byte{
		0x64, 0x00, 0x00, 0x00, // HP = 100
		0x00, 0x00, 0xC0, 0x3F, // Speed = 1.5
		0x03, 0x00, 0x00, 0x00, // Info.Level = 3, inlined
	}
	if !bytes.Equal(got, want) {
		t.Fatalf("encode = % X, want % X", got, want)
	}

	decoded := &Player{}
	r := NewReader(got)
	if err := edge.Decode(r, decoded); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if decoded.HP != 100 || decoded.Info.Level != 3 {
		t.Errorf("decoded = %+v", decoded)
	}
	if !r.IsEmpty() {
		t.Errorf("cursor did not land exactly at the end")
	}
}

// ============================================================== §6 - Array<T>

// teamSample and teamGoldenBytes are the exact same schema and bytes asserted
// in tests/generated.rs (Rust) and tests/csharp/GeneratedTests.cs (C#), and
// confirmed by a real Godot run over tests/fixtures/gdscript/team.gd.
func teamSample() *Team {
	return &Team{
		Scores:  []uint32{10, 20, 30},
		Names:   []string{"alice", "bob"},
		Players: []PlayerInfo{{Level: 3}, {Level: 7}},
	}
}

func teamGoldenBytes() []byte {
	return []byte{
		0x03, 0x00, 0x00, 0x00, // Scores count = 3
		0x0A, 0x00, 0x00, 0x00, // Scores[0] = 10
		0x14, 0x00, 0x00, 0x00, // Scores[1] = 20
		0x1E, 0x00, 0x00, 0x00, // Scores[2] = 30
		0x02, 0x00, 0x00, 0x00, // Names count = 2
		0x05, 0x00, 0x00, 0x00, 'a', 'l', 'i', 'c', 'e', // Names[0]
		0x03, 0x00, 0x00, 0x00, 'b', 'o', 'b', // Names[1]
		0x02, 0x00, 0x00, 0x00, // Players count = 2
		0x03, 0x00, 0x00, 0x00, // Players[0].Level = 3, inlined
		0x07, 0x00, 0x00, 0x00, // Players[1].Level = 7, inlined
	}
}

// h.md §6 - Array<T> is a UInt32 count followed by that many elements, no
// per-element length prefix. Same bytes as the Rust and C# backends' identical
// tests.
func TestArrayOfScalarStringAndModelMatchesTheGoldenBytes(t *testing.T) {
	edge := TeamEdgeCodec{}
	w := NewWriter()
	edge.Encode(w, teamSample())

	if got, want := w.Bytes(), teamGoldenBytes(); !bytes.Equal(got, want) {
		t.Fatalf("encode = % X, want % X", got, want)
	}
}

func TestArrayRoundTripsIncludingNestedModelElements(t *testing.T) {
	edge := TeamEdgeCodec{}
	encoded := teamGoldenBytes()

	decoded := &Team{}
	r := NewReader(encoded)
	if err := edge.Decode(r, decoded); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if !r.IsEmpty() {
		t.Errorf("cursor did not land exactly at the end")
	}
	if !bytes.Equal(uint32sToBytes(decoded.Scores), uint32sToBytes(teamSample().Scores)) {
		t.Errorf("Scores = %v", decoded.Scores)
	}
	if len(decoded.Players) != 2 || decoded.Players[0].Level != 3 || decoded.Players[1].Level != 7 {
		t.Errorf("Players = %+v", decoded.Players)
	}
}

func uint32sToBytes(values []uint32) []byte {
	out := make([]byte, 0, len(values)*4)
	for _, v := range values {
		out = append(out, byte(v), byte(v>>8), byte(v>>16), byte(v>>24))
	}
	return out
}

// An empty Array<T> is just its UInt32 count of zero.
func TestAnEmptyArrayIsJustItsZeroCount(t *testing.T) {
	edge := TeamEdgeCodec{}
	w := NewWriter()
	edge.Encode(w, &Team{})

	want := []byte{0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0}
	if got := w.Bytes(); !bytes.Equal(got, want) {
		t.Fatalf("encode = % X, want % X", got, want)
	}

	decoded := &Team{Scores: []uint32{1}}
	if err := edge.Decode(NewReader(w.Bytes()), decoded); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if len(decoded.Scores) != 0 {
		t.Errorf("Scores = %v, want empty", decoded.Scores)
	}
}

// Limits.MaxArrayCount is checked before an element is allocated, the same
// guard MaxStringLength/MaxBytesLength already give scalar fields.
func TestAnArrayCountOverTheLimitIsRejectedBeforeAllocating(t *testing.T) {
	w := NewWriter()
	w.WriteArrayCount(5)

	limits := UnlimitedLimits
	limits.MaxArrayCount = 2
	r := NewReaderWithLimits(w.Bytes(), limits)

	if _, err := r.ReadArrayCount(); err == nil {
		t.Fatal("expected an error for an array count over the limit")
	}
}

// ============================================================ §4 - primitives

// h.md §4 - each network type maps to the runtime method RFC-0002 defines,
// and these are the bytes that method writes.
func TestEveryPrimitiveMatchesTheSpecification(t *testing.T) {
	all := EveryPrimitiveAllCodec{}
	value := &EveryPrimitive{
		Flag: true,
		A:    -1,
		B:    255,
		C:    -1,
		D:    300,
		E:    -1,
		F:    0x12345678,
		G:    -1,
		H:    1,
		I:    1.5,
		J:    1.0,
		K:    "中",
		L:    []byte{0xFF, 0xFE},
	}

	w := NewWriter()
	all.Encode(w, value)
	got := w.Bytes()

	want := []byte{
		0x01,       // bool
		0xFF,       // i8 -1
		0xFF,       // u8 255
		0xFF, 0xFF, // i16 -1
		0x2C, 0x01, // u16 300
		0xFF, 0xFF, 0xFF, 0xFF, // i32 -1
		0x78, 0x56, 0x34, 0x12, // u32 0x12345678 - the endianness vector
		0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, // i64 -1
		0x01, 0, 0, 0, 0, 0, 0, 0, // u64 1
		0x00, 0x00, 0xC0, 0x3F, // f32 1.5
		0, 0, 0, 0, 0, 0, 0xF0, 0x3F, // f64 1.0
		0x03, 0, 0, 0, 0xE4, 0xB8, 0xAD, // string "中" - 3 bytes, not 1 char
		0x02, 0, 0, 0, 0xFF, 0xFE, // bytes
	}
	if !bytes.Equal(got, want) {
		t.Fatalf("encode = % X\nwant    = % X", got, want)
	}
}

// ================================================ the runtime the file carries

// The generated file carries a conforming decoder, not a permissive one.
func TestTheEmbeddedRuntimeRejectsMalformedInput(t *testing.T) {
	edge := DeviceStateEdgeCodec{}

	if _, err := NewReader([]byte{0x02}).ReadBool(); err == nil {
		t.Error("expected an error decoding an invalid bool")
	} else if de, ok := err.(*DecodeError); !ok || de.Kind != "invalid_bool" {
		t.Errorf("wrong error: %v", err)
	}

	value := &DeviceState{}
	if err := edge.Decode(NewReader([]byte{0x2A, 0x00, 0x00}), value); err == nil {
		t.Error("expected an error decoding a truncated value")
	} else if de, ok := err.(*DecodeError); !ok || de.Kind != "unexpected_eof" || de.Needed != 4 {
		t.Errorf("wrong error: %v", err)
	}

	unity := DeviceStateUnityCodec{}
	badUTF8 := []byte{0x2A, 0, 0, 0, 0x02, 0, 0, 0, 0xFF, 0xFE}
	if err := unity.Decode(NewReader(badUTF8), value); err == nil {
		t.Error("expected an error decoding invalid utf-8")
	} else if de, ok := err.(*DecodeError); !ok || de.Kind != "invalid_utf8" {
		t.Errorf("wrong error: %v", err)
	}
}

// Limits reaches the generated code unchanged, so a caller can bound what an
// untrusted stream may allocate.
func TestTheEmbeddedLimitsApplyToGeneratedDecode(t *testing.T) {
	unity := DeviceStateUnityCodec{}

	data := append([]byte{0x2A, 0, 0, 0, 0x10, 0, 0, 0}, []byte("0123456789abcdef")...)

	limits := Limits{MaxStringLen: 8, MaxBytesLen: UnlimitedLimits.MaxBytesLen}
	value := &DeviceState{}
	err := unity.Decode(NewReaderWithLimits(data, limits), value)
	if err == nil {
		t.Fatal("expected a length-overflow error")
	}
	de, ok := err.(*DecodeError)
	if !ok || de.Kind != "length_overflow" || de.Length != 16 || de.Limit != 8 {
		t.Errorf("wrong error: %v", err)
	}

	// The default is permissive: the same bytes decode fine.
	permissive := &DeviceState{}
	if err := unity.Decode(NewReader(data), permissive); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if permissive.Name != "0123456789abcdef" {
		t.Errorf("Name = %q", permissive.Name)
	}
}

// Floats are written as raw bits: -0.0 stays distinct from 0.0, and nothing is
// canonicalized.
func TestFloatsKeepTheirBits(t *testing.T) {
	edge := DeviceStateEdgeCodec{}

	negative := encodeWith(t, &DeviceState{Temperature: negZero()}, edge.Encode)
	positive := encodeWith(t, &DeviceState{Temperature: 0.0}, edge.Encode)

	if bytes.Equal(negative, positive) {
		t.Fatal("-0.0 and 0.0 produced the same bytes")
	}

	value := &DeviceState{}
	if err := edge.Decode(NewReader(negative), value); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if !math.Signbit(float64(value.Temperature)) {
		t.Errorf("decoded Temperature lost its sign: %v", value.Temperature)
	}
}

func negZero() float32 { return float32(math.Copysign(0, -1)) }

// ================================================================ edge cases

// h.md §9's "no field joined" case - a declared codec is generated even when
// no field joined it.
func TestACodecNoFieldJoinedIsStillGenerated(t *testing.T) {
	lonely := NoFieldsJoinedLonelyCodec{}
	w := NewWriter()
	lonely.Encode(w, &NoFieldsJoined{ID: 1})
	if w.Len() != 0 {
		t.Errorf("a model with no routed field occupies zero bytes, got %d", w.Len())
	}

	value := &NoFieldsJoined{}
	if err := lonely.Decode(NewReader(nil), value); err != nil {
		t.Fatalf("decode: %v", err)
	}
}

// A struct nothing marks is not a model, and its neighbours are unaffected.
func TestAnUnmarkedStructDoesNotDisturbTheNextModel(t *testing.T) {
	_ = NotAModel{Whatever: 1}

	edge := AfterTheUnmarkedStructEdgeCodec{}
	got := encodeWith2(edge.Encode, &AfterTheUnmarkedStruct{Value: 5})
	want := []byte{0x05, 0x00, 0x00, 0x00}
	if !bytes.Equal(got, want) {
		t.Errorf("encode = % X, want % X", got, want)
	}
}

func encodeWith2[T any](encode func(*Writer, *T), value *T) []byte {
	w := NewWriter()
	encode(w, value)
	return w.Bytes()
}

// Encoding the same value twice produces the same bytes.
func TestEncodingIsDeterministic(t *testing.T) {
	edge := DeviceStateEdgeCodec{}
	a := encodeWith(t, sample(), edge.Encode)
	b := encodeWith(t, sample(), edge.Encode)
	if !bytes.Equal(a, b) {
		t.Error("encoding the same value twice produced different bytes")
	}
}
