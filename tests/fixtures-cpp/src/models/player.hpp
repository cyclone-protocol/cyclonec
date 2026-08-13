// player.hpp is the annotated source the C++ integration tests exercise - the
// C++ counterpart of tests/fixtures-cs/src/models/Player.cs: three models
// annotated in place, the generated codecs compiled against these same
// types, and the model RFC-0002 §9.1's version-skew tests decode.
#pragma once

#include <cstdint>
#include <string>
#include <vector>

#include "cyclone.h"

namespace models {

/// A field whose network type is another model.
CYCLONE_MODEL
CYCLONE_CODEC("edge")
struct PlayerInfo
{
    CYCLONE_FIELD(u32)
    CYCLONE_CODEC("edge")
    uint32_t Level = 0;
};

/// The model RFC-0002 §9.1 is tested against: three fields, and a version
/// that appends a fourth.
CYCLONE_MODEL
CYCLONE_CODEC("edge", "unity")
struct Player
{
    CYCLONE_FIELD(u32)
    CYCLONE_CODEC("edge", "unity")
    uint32_t Id = 0;

    CYCLONE_FIELD(f32)
    CYCLONE_CODEC("edge")
    float X = 0.0f;

    CYCLONE_FIELD(f32)
    CYCLONE_CODEC("edge")
    float Y = 0.0f;

    /// A network field in no codec: it is written by none of them.
    CYCLONE_FIELD(u32)
    uint32_t Unrouted = 0;

    /// Not a network field at all. Logic and caches stay off the wire.
    std::string Cache;
};

/// Holds composites: an array of primitives, an array of models, and a
/// nested model.
CYCLONE_MODEL
CYCLONE_CODEC("edge")
struct Team
{
    CYCLONE_FIELD(PlayerInfo)
    CYCLONE_CODEC("edge")
    PlayerInfo Captain;

    CYCLONE_FIELD(Array<string>)
    CYCLONE_CODEC("edge")
    std::vector<std::string> Tags;

    CYCLONE_FIELD(Array<u32>)
    CYCLONE_CODEC("edge")
    std::vector<uint32_t> Scores;

    CYCLONE_FIELD(Array<PlayerInfo>)
    CYCLONE_CODEC("edge")
    std::vector<PlayerInfo> Roster;
};

}  // namespace models
