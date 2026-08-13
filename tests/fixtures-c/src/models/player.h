#pragma once

/* A C fixture model, in the exact style the brief's own DeviceState example
 * uses: a plain tagged `struct Name { ... };`, no `typedef`. Every generated
 * reference to one of these types is spelled `struct Name` for that reason -
 * see generator::c::struct_type's doc comment. */

#include "cyclone.h"

/* CycloneBytes / CycloneArray_* are generated types (arrays.h, runtime.h),
 * not standard ones - unlike C++'s std::string/std::vector, a C model that
 * uses `bytes` or `Array<T>` fields has a real build-order dependency on a
 * previous `cyclonec generate` run having produced them. This fixture's
 * generated tree is committed, so that dependency is already satisfied. */
#include "../generated/arrays.h"
#include "../generated/runtime.h"

#include <stdint.h>

CYCLONE_MODEL
CYCLONE_CODEC("edge")
struct PlayerInfo {
    CYCLONE_FIELD(u32)
    CYCLONE_CODEC("edge")
    uint32_t Level;
};

CYCLONE_MODEL
CYCLONE_CODEC("edge", "unity")
struct Player {
    CYCLONE_FIELD(u32)
    CYCLONE_CODEC("edge", "unity")
    uint32_t Id;

    CYCLONE_FIELD(f32)
    CYCLONE_CODEC("edge")
    float X;

    CYCLONE_FIELD(f32)
    CYCLONE_CODEC("edge")
    float Y;

    CYCLONE_FIELD(string)
    CYCLONE_CODEC("edge", "unity")
    const char *Name;

    CYCLONE_FIELD(bytes)
    CYCLONE_CODEC("edge")
    CycloneBytes Payload;

    /* Not on the wire at all. */
    int Cache;
};

CYCLONE_MODEL
CYCLONE_CODEC("edge")
struct Team {
    CYCLONE_FIELD(PlayerInfo)
    CYCLONE_CODEC("edge")
    struct PlayerInfo Captain;

    CYCLONE_FIELD(Array<string>)
    CYCLONE_CODEC("edge")
    CycloneArray_string Tags;

    CYCLONE_FIELD(Array<u32>)
    CYCLONE_CODEC("edge")
    CycloneArray_u32 Scores;

    CYCLONE_FIELD(Array<PlayerInfo>)
    CYCLONE_CODEC("edge")
    CycloneArray_PlayerInfo Roster;
};
