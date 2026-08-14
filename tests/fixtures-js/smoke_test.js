// Not part of the generated tree or the fixture cyclonec reads - a
// throwaway smoke test run by hand (see the CI workflow) to prove the
// generated codecs actually work, since no JavaScript step runs inside
// `cargo test` itself.

import assert from "node:assert/strict";

import { Player, PlayerInfo, Team } from "./src/models/player.js";
import { DeviceState, Telemetry } from "./src/models/device_state.js";
import { EveryPrimitive } from "./src/models/every_primitive.js";
import { Reader, Writer } from "./src/generated/runtime.js";
import { PlayerEdgeCodec } from "./src/generated/player_edge.js";
import { PlayerUnityCodec } from "./src/generated/player_unity.js";
import { TeamEdgeCodec } from "./src/generated/team_edge.js";
import { DeviceStateEdgeCodec } from "./src/generated/device_state_edge.js";
import { DeviceStateUnityCodec } from "./src/generated/device_state_unity.js";
import { TelemetryEdgeCodec } from "./src/generated/telemetry_edge.js";
import { EveryPrimitiveEdgeCodec } from "./src/generated/every_primitive_edge.js";
import {
    CYCLONE_SCHEMA_FINGERPRINT,
    CYCLONE_MESSAGES,
    cycloneMessage,
    cycloneHandshake,
    CycloneHandshake,
    PLAYER_EDGE_MESSAGE_ID,
    PLAYER_EDGE_FINGERPRINT,
} from "./src/generated/handshake.js";

function encode(value, fn) {
    const writer = new Writer();
    fn(writer, value);
    return writer.toUint8Array();
}

// RFC-0002: fields back to back, Little Endian, no padding.
{
    const player = new Player();
    player.Id = 100;
    player.X = 10.5;
    player.Y = 20.0;

    const bytes = encode(player, PlayerEdgeCodec.encode);
    assert.deepEqual(Array.from(bytes), [
        0x64, 0x00, 0x00, 0x00, // id = 100, u32 LE
        0x00, 0x00, 0x28, 0x41, // x = 10.5, raw IEEE 754 bits
        0x00, 0x00, 0xa0, 0x41, // y = 20.0
    ]);

    const decoded = new Player();
    PlayerEdgeCodec.decode(new Reader(bytes), decoded);
    assert.equal(decoded.Id, 100);
    assert.equal(decoded.X, 10.5);
    assert.equal(decoded.Y, 20.0);
}

// A nested model is inline, and a composite round-trips.
{
    const team = new Team();
    team.Captain.Level = 7;
    team.Tags = ["red", "blue"];
    team.Scores = [10, 20, 30];
    const roster = new PlayerInfo();
    roster.Level = 3;
    team.Roster = [roster];

    const bytes = encode(team, TeamEdgeCodec.encode);
    const decoded = new Team();
    TeamEdgeCodec.decode(new Reader(bytes), decoded);

    assert.equal(decoded.Captain.Level, 7);
    assert.deepEqual(decoded.Tags, ["red", "blue"]);
    assert.deepEqual(decoded.Scores, [10, 20, 30]);
    assert.equal(decoded.Roster.length, 1);
    assert.equal(decoded.Roster[0].Level, 3);
}

// RFC-0002 §9.1: an old writer's payload zeroes the fields it never wrote.
{
    const oldPlayer = new Player();
    oldPlayer.Id = 42;
    const idOnly = new Writer();
    idOnly.writeU32(oldPlayer.Id);

    const decoded = new Player();
    decoded.X = 99.0; // proves the decoder actually zeroes it
    PlayerEdgeCodec.decode(new Reader(idOnly.toUint8Array()), decoded);
    assert.equal(decoded.Id, 42);
    assert.equal(decoded.X, 0.0);
    assert.equal(decoded.Y, 0.0);
}

// A truncated field (not absent - the stream ends *inside* X) is an error.
{
    const truncated = new Writer();
    truncated.writeU32(1);
    truncated.writeU8(0); // one byte of what should be a 4-byte f32
    assert.throws(() => {
        PlayerEdgeCodec.decode(new Reader(truncated.toUint8Array()), new Player());
    });
}

// Two codecs of one model write only the fields that named them.
{
    const state = new DeviceState();
    state.Id = 42;
    state.Temperature = 21.5;
    state.DisplayName = "sensor-1";

    const edgeBytes = encode(state, DeviceStateEdgeCodec.encode);
    assert.deepEqual(Array.from(edgeBytes), [0x2a, 0x00, 0x00, 0x00, 0x00, 0x00, 0xac, 0x41]);

    const unityBytes = encode(state, DeviceStateUnityCodec.encode);
    const decoded = new DeviceState();
    DeviceStateUnityCodec.decode(new Reader(unityBytes), decoded);
    assert.equal(decoded.Id, 42);
    assert.equal(decoded.DisplayName, "sensor-1");
}

// A `string` length counts bytes, not characters.
{
    const state = new DeviceState();
    state.DisplayName = "héllo";
    const bytes = encode(state, DeviceStateUnityCodec.encode);
    assert.equal(bytes[4], 6);
    assert.equal(bytes.length, 4 + 4 + 6);
}

// A `u64` field is a `bigint`, exact past 2^53.
{
    const telemetry = new Telemetry();
    telemetry.Sequence = 0x0102030405060708n;
    const bytes = encode(telemetry, TelemetryEdgeCodec.encode);
    const decoded = new Telemetry();
    TelemetryEdgeCodec.decode(new Reader(bytes), decoded);
    assert.equal(decoded.Sequence, 0x0102030405060708n);
}

// Every primitive round-trips at its specified width.
{
    const value = new EveryPrimitive();
    value.Flag = true;
    value.Tiny = -2;
    value.Byte = 0xff;
    value.Small = -300;
    value.Port = 8080;
    value.Offset = -1;
    value.Count = 4294967295;
    value.Delta = -2n;
    value.Sequence = 1n;
    value.Ratio = 1.0;
    value.Precise = 2.0;
    value.Label = "ok";
    value.Blob = new Uint8Array([0xde, 0xad]);

    const bytes = encode(value, EveryPrimitiveEdgeCodec.encode);
    const decoded = new EveryPrimitive();
    EveryPrimitiveEdgeCodec.decode(new Reader(bytes), decoded);

    assert.equal(decoded.Flag, true);
    assert.equal(decoded.Tiny, -2);
    assert.equal(decoded.Byte, 0xff);
    assert.equal(decoded.Small, -300);
    assert.equal(decoded.Port, 8080);
    assert.equal(decoded.Offset, -1);
    assert.equal(decoded.Count, 4294967295);
    assert.equal(decoded.Delta, -2n);
    assert.equal(decoded.Sequence, 1n);
    assert.equal(decoded.Ratio, 1.0);
    assert.equal(decoded.Precise, 2.0);
    assert.equal(decoded.Label, "ok");
    assert.deepEqual(Array.from(decoded.Blob), [0xde, 0xad]);
}

// The handshake and the codec's own constants agree.
{
    assert.equal(PlayerEdgeCodec.MESSAGE_ID, PLAYER_EDGE_MESSAGE_ID);
    assert.equal(PlayerEdgeCodec.FINGERPRINT, PLAYER_EDGE_FINGERPRINT);

    const peer = CYCLONE_MESSAGES.map((message) => [message.id, message.fingerprint]);
    assert.equal(cycloneHandshake(CYCLONE_SCHEMA_FINGERPRINT, peer), CycloneHandshake.CURRENT);
    assert.notEqual(cycloneMessage(PLAYER_EDGE_MESSAGE_ID), undefined);
    assert.equal(cycloneMessage(0), undefined);

    const rejected = peer.map(([id, fingerprint]) =>
        id === PLAYER_EDGE_MESSAGE_ID ? [id, fingerprint ^ 1n] : [id, fingerprint],
    );
    assert.equal(cycloneHandshake(0xdeadbeef_00000000n, rejected), CycloneHandshake.REJECT);
}

// PlayerUnityCodec exists and is distinct from PlayerEdgeCodec.
assert.notEqual(PlayerUnityCodec.FINGERPRINT, PlayerEdgeCodec.FINGERPRINT);

console.log("ok - all JavaScript fixture smoke tests passed");
