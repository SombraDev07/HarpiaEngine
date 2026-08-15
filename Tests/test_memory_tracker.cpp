#include <doctest/doctest.h>

#include "Core/Memory/MemoryTracker.h"

#include <cstdint>
#include <string_view>
#include <thread>
#include <vector>

using namespace harpia;

// Counters are process-global, so every assertion here is on a delta rather
// than an absolute — other tests and the job system allocate concurrently.

TEST_CASE("MemoryTracker records allocation and free")
{
    const MemStats before = MemoryTracker::stats(MemTag::Assets);

    MemoryTracker::recordAlloc(MemTag::Assets, 1024);

    const MemStats mid = MemoryTracker::stats(MemTag::Assets);
    CHECK(mid.current == before.current + 1024);
    CHECK(mid.allocs == before.allocs + 1);

    MemoryTracker::recordFree(MemTag::Assets, 1024);

    const MemStats after = MemoryTracker::stats(MemTag::Assets);
    CHECK(after.current == before.current);
    CHECK(after.frees == before.frees + 1);
}

TEST_CASE("MemoryTracker peak never decreases")
{
    MemoryTracker::recordAlloc(MemTag::Audio, 4096);
    const MemStats peakHigh = MemoryTracker::stats(MemTag::Audio);
    MemoryTracker::recordFree(MemTag::Audio, 4096);
    const MemStats afterFree = MemoryTracker::stats(MemTag::Audio);

    CHECK(afterFree.peak == peakHigh.peak);
    CHECK(afterFree.peak >= 4096);
    CHECK(afterFree.current < peakHigh.current);
}

TEST_CASE("MemoryTracker tags are independent")
{
    const MemStats physicsBefore = MemoryTracker::stats(MemTag::Physics);
    const MemStats renderBefore  = MemoryTracker::stats(MemTag::Render);

    MemoryTracker::recordAlloc(MemTag::Physics, 512);

    CHECK(MemoryTracker::stats(MemTag::Physics).current == physicsBefore.current + 512);
    CHECK(MemoryTracker::stats(MemTag::Render).current == renderBefore.current);

    MemoryTracker::recordFree(MemTag::Physics, 512);
}

TEST_CASE("MemoryTracker is thread safe")
{
    const MemStats before = MemoryTracker::stats(MemTag::Scene);

    constexpr int kThreads       = 8;
    constexpr int kPerThread     = 2000;
    constexpr std::size_t kBytes = 64;

    std::vector<std::thread> threads;
    threads.reserve(kThreads);

    for (int t = 0; t < kThreads; ++t) {
        threads.emplace_back([] {
            for (int i = 0; i < kPerThread; ++i) {
                MemoryTracker::recordAlloc(MemTag::Scene, kBytes);
            }
            for (int i = 0; i < kPerThread; ++i) {
                MemoryTracker::recordFree(MemTag::Scene, kBytes);
            }
        });
    }
    for (std::thread& t : threads) {
        t.join();
    }

    const MemStats after = MemoryTracker::stats(MemTag::Scene);
    CHECK(after.current == before.current);
    CHECK(after.allocs == before.allocs + kThreads * kPerThread);
    CHECK(after.frees == before.frees + kThreads * kPerThread);
}

TEST_CASE("tagged allocate/deallocate round trips")
{
    const MemStats before = MemoryTracker::stats(MemTag::Editor);

    void* ptr = harpia::allocate(256, 64, MemTag::Editor);
    REQUIRE(ptr != nullptr);
    CHECK(reinterpret_cast<std::uintptr_t>(ptr) % 64 == 0);
    CHECK(MemoryTracker::stats(MemTag::Editor).current > before.current);

    harpia::deallocate(ptr, 256, 64, MemTag::Editor);
    CHECK(MemoryTracker::stats(MemTag::Editor).current == before.current);
}

TEST_CASE("toString covers every tag")
{
    for (std::uint8_t i = 0; i < static_cast<std::uint8_t>(MemTag::Count); ++i) {
        CHECK(std::string_view{toString(static_cast<MemTag>(i))} != "Unknown");
    }
}
