// Not part of the generated tree or the fixture cyclonec reads - a
// throwaway smoke test compiled by hand (see README's C++ section) to prove
// the generated headers actually build and round-trip, since no C++
// toolchain step runs inside `cargo test` itself.
#include <cassert>
#include <cstdio>

#include "generated/handshake.hpp"
#include "generated/player_edge.hpp"
#include "generated/player_info_edge.hpp"
#include "generated/player_unity.hpp"
#include "generated/team_edge.hpp"

int main() {
    using namespace generated;

    models::Team team;
    team.Captain.Level = 7;
    team.Tags = {"red", "blue"};
    team.Scores = {10, 20, 30};
    models::PlayerInfo roster_entry;
    roster_entry.Level = 3;
    team.Roster = {roster_entry};

    Writer writer;
    TeamEdgeCodec::encode(writer, team);

    Reader reader(writer.bytes().data(), writer.bytes().size());
    models::Team decoded;
    DecodeError error = TeamEdgeCodec::decode(reader, decoded);
    assert(error.ok());
    assert(decoded.Captain.Level == 7);
    assert(decoded.Tags.size() == 2 && decoded.Tags[0] == "red" && decoded.Tags[1] == "blue");
    assert(decoded.Scores.size() == 3 && decoded.Scores[1] == 20);
    assert(decoded.Roster.size() == 1 && decoded.Roster[0].Level == 3);

    // RFC-0002 section 9.1: a payload from an older writer that only wrote
    // `Id` still decodes, with `X`/`Y`/`Unrouted` taking their zero value.
    models::Player old_player;
    old_player.Id = 42;
    Writer id_only;
    id_only.write_u32(old_player.Id);

    Reader skew_reader(id_only.bytes().data(), id_only.bytes().size());
    models::Player decoded_player;
    decoded_player.X = 99.0f;  // proves the decoder actually zeroes it
    DecodeError player_error = PlayerEdgeCodec::decode(skew_reader, decoded_player);
    assert(player_error.ok());
    assert(decoded_player.Id == 42);
    assert(decoded_player.X == 0.0f);
    assert(decoded_player.Y == 0.0f);

    // A truncated field (not absent - the stream ends *inside* X) is an
    // error, not a zero.
    Writer truncated;
    truncated.write_u32(1);
    truncated.write_u8(0);  // one byte of what should be a 4-byte f32
    Reader truncated_reader(truncated.bytes().data(), truncated.bytes().size());
    models::Player truncated_player;
    DecodeError truncated_error = PlayerEdgeCodec::decode(truncated_reader, truncated_player);
    assert(!truncated_error.ok());
    assert(truncated_error.kind == DecodeError::Kind::UnexpectedEof);

    // The handshake: identical schema is Current, and a peer that only knows
    // an older subset of messages with matching fingerprints is Outdated.
    CycloneHandshake current = cyclone_handshake(CYCLONE_SCHEMA_FINGERPRINT, {});
    assert(current == CycloneHandshake::Current);

    CycloneHandshake outdated = cyclone_handshake(
        CYCLONE_SCHEMA_FINGERPRINT + 1,
        {CyclonePeerMessage{PLAYER_EDGE_MESSAGE_ID, PLAYER_EDGE_FINGERPRINT}});
    assert(outdated == CycloneHandshake::Outdated);

    CycloneHandshake reject = cyclone_handshake(
        CYCLONE_SCHEMA_FINGERPRINT + 1,
        {CyclonePeerMessage{PLAYER_EDGE_MESSAGE_ID, PLAYER_EDGE_FINGERPRINT + 1}});
    assert(reject == CycloneHandshake::Reject);

    std::puts("cpp fixture smoke test: ok");
    return 0;
}
