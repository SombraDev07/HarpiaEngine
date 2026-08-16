// Harpia Engine — Recast/Detour allocations carry MemTag::Scene
//
// Navmesh data is scene geometry. Without this hook the baker's heaps are
// invisible to MemoryTracker.
#pragma once

namespace harpia::nav {

void installRecastAllocator();

} // namespace harpia::nav
