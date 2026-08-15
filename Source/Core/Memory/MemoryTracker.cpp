#include "Core/Memory/MemoryTracker.h"

#include <atomic>
#include <array>
#include <cstdlib>
#include <new>

namespace harpia {
namespace {

struct Counters {
    std::atomic<std::size_t>   current{0};
    std::atomic<std::size_t>   peak{0};
    std::atomic<std::uint64_t> allocs{0};
    std::atomic<std::uint64_t> frees{0};
};

constexpr std::size_t kTagCount = static_cast<std::size_t>(MemTag::Count);

// Function-local static: no static-init-order problem, and allocations that
// happen during static construction still land somewhere valid.
Counters* counters() noexcept
{
    static std::array<Counters, kTagCount> instance{};
    return instance.data();
}

constexpr std::size_t index(MemTag tag) noexcept
{
    return static_cast<std::size_t>(tag);
}

} // namespace

const char* toString(MemTag tag) noexcept
{
    switch (tag) {
        case MemTag::General: return "General";
        case MemTag::Scratch: return "Scratch";
        case MemTag::Render:  return "Render";
        case MemTag::Assets:  return "Assets";
        case MemTag::Scene:   return "Scene";
        case MemTag::Physics: return "Physics";
        case MemTag::Audio:   return "Audio";
        case MemTag::Jobs:    return "Jobs";
        case MemTag::Editor:  return "Editor";
        case MemTag::Count:   break;
    }
    return "Unknown";
}

void MemoryTracker::recordAlloc(MemTag tag, std::size_t bytes) noexcept
{
    Counters& c = counters()[index(tag)];
    const std::size_t now = c.current.fetch_add(bytes, std::memory_order_relaxed) + bytes;
    c.allocs.fetch_add(1, std::memory_order_relaxed);

    // Raise the high-water mark. Losing a race here only means another thread
    // already published a larger value, so the CAS loop terminates.
    std::size_t observed = c.peak.load(std::memory_order_relaxed);
    while (now > observed &&
           !c.peak.compare_exchange_weak(observed, now,
                                         std::memory_order_relaxed,
                                         std::memory_order_relaxed)) {
        // observed reloaded by compare_exchange_weak
    }
}

void MemoryTracker::recordFree(MemTag tag, std::size_t bytes) noexcept
{
    Counters& c = counters()[index(tag)];
    c.current.fetch_sub(bytes, std::memory_order_relaxed);
    c.frees.fetch_add(1, std::memory_order_relaxed);
}

MemStats MemoryTracker::stats(MemTag tag) noexcept
{
    const Counters& c = counters()[index(tag)];
    MemStats s;
    s.current = c.current.load(std::memory_order_relaxed);
    s.peak    = c.peak.load(std::memory_order_relaxed);
    s.allocs  = c.allocs.load(std::memory_order_relaxed);
    s.frees   = c.frees.load(std::memory_order_relaxed);
    return s;
}

std::size_t MemoryTracker::totalCurrent() noexcept
{
    std::size_t total = 0;
    for (std::size_t i = 0; i < kTagCount; ++i) {
        total += counters()[i].current.load(std::memory_order_relaxed);
    }
    return total;
}

std::size_t MemoryTracker::totalPeak() noexcept
{
    std::size_t total = 0;
    for (std::size_t i = 0; i < kTagCount; ++i) {
        total += counters()[i].peak.load(std::memory_order_relaxed);
    }
    return total;
}

void MemoryTracker::resetAll() noexcept
{
    for (std::size_t i = 0; i < kTagCount; ++i) {
        Counters& c = counters()[i];
        c.current.store(0, std::memory_order_relaxed);
        c.peak.store(0, std::memory_order_relaxed);
        c.allocs.store(0, std::memory_order_relaxed);
        c.frees.store(0, std::memory_order_relaxed);
    }
}

bool MemoryTracker::isClean() noexcept
{
    return totalCurrent() == 0;
}

void* allocate(std::size_t bytes, std::size_t alignment, MemTag tag)
{
    if (bytes == 0) {
        return nullptr;
    }

    // operator new[] with alignment requires size to be a multiple of alignment.
    const std::size_t rounded = (bytes + alignment - 1) & ~(alignment - 1);

    void* ptr = ::operator new(rounded, std::align_val_t{alignment});
    MemoryTracker::recordAlloc(tag, rounded);
    return ptr;
}

void deallocate(void* ptr, std::size_t bytes, std::size_t alignment, MemTag tag) noexcept
{
    if (ptr == nullptr) {
        return;
    }

    const std::size_t rounded = (bytes + alignment - 1) & ~(alignment - 1);

    ::operator delete(ptr, rounded, std::align_val_t{alignment});
    MemoryTracker::recordFree(tag, rounded);
}

} // namespace harpia
