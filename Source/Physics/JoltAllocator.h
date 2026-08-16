// Harpia Engine — Jolt allocations carry MemTag::Physics
//
// Jolt lets us replace malloc. Without this hook its heaps are invisible to
// MemoryTracker, which is exactly the retrofit the roadmap called expensive.
#pragma once

namespace harpia::phys {

void installJoltAllocator();

} // namespace harpia::phys
