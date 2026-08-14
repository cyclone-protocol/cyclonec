// The brief's own example (issue.md §1), verified parsing correctly and
// generating a working codec - see tests/cli.rs's
// `js_the_brief_s_device_state_example_parses_and_generates`.

// CYCLONE_MODEL
// CYCLONE_CODEC("edge", "unity")
export class DeviceState {
    // CYCLONE_FIELD(u32)
    // CYCLONE_CODEC("edge", "unity")
    Id = 0;

    // CYCLONE_FIELD(f32)
    // CYCLONE_CODEC("edge")
    Temperature = 0;

    // CYCLONE_FIELD(string)
    // CYCLONE_CODEC("unity")
    DisplayName = "";
}

// Codec names the generator has never heard of, and the PascalCase they
// turn into - and a 64-bit field, which is a `bigint` here and nowhere else
// in this fixture.
// CYCLONE_MODEL
// CYCLONE_CODEC("edge", "orange_pi")
export class Telemetry {
    // CYCLONE_FIELD(u64)
    // CYCLONE_CODEC("edge", "orange_pi")
    Sequence = 0n;
}
