// Harpia Engine — memory tracking
//
// Every allocation carries a subsystem tag. See HARPIA-ROADMAP.md 1.2:
// tracking is cheap to add now and mechanical, boring work to retrofit later.
#pragma once

#include <cstddef>
#include <cstdint>

namespace harpia {

enum class MemTag : std::uint8_t {
    General = 0,
    Scratch,   // frame arenas
    Render,
    Assets,
    Scene,
    Physics,
    Audio,
    Jobs,
    Editor,
    Count
};

[[nodiscard]] const char* toString(MemTag tag) noexcept;

struct MemStats {
    std::size_t   current = 0;  // bytes live right now
    std::size_t   peak    = 0;  // high-water mark since reset
    std::uint64_t allocs  = 0;
    std::uint64_t frees   = 0;
};

// Thread-safe counters. Recording is two atomics; safe to call from any thread.
class MemoryTracker {
public:
    static void recordAlloc(MemTag tag, std::size_t bytes) noexcept;
    static void recordFree(MemTag tag, std::size_t bytes) noexcept;

    [[nodiscard]] static MemStats     stats(MemTag tag) noexcept;
    [[nodiscard]] static std::size_t  totalCurrent() noexcept;
    [[nodiscard]] static std::size_t  totalPeak() noexcept;

    // Test-only: clears every counter.
    static void resetAll() noexcept;

    // Returns true when every tag has current == 0. Used by leak tests.
    [[nodiscard]] static bool isClean() noexcept;
};

// Tagged raw allocation. Prefer Arena or Pool; this is for the rare
// long-lived, dynamically-sized block.
[[nodiscard]] void* allocate(std::size_t bytes, std::size_t alignment, MemTag tag);
void deallocate(void* ptr, std::size_t bytes, std::size_t alignment, MemTag tag) noexcept;

} // namespace harpia
