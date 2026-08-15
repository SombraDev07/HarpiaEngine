#include <doctest/doctest.h>

#include "Core/Threading/JobSystem.h"

#include <array>
#include <atomic>
#include <numeric>
#include <vector>

using namespace harpia;

TEST_CASE("job system starts workers")
{
    JobSystem& js = JobSystem::get();
    CHECK(js.initialized());
    CHECK(js.workerCount() >= 1);
}

TEST_CASE("a submitted job runs before wait returns")
{
    JobSystem& js = JobSystem::get();

    std::atomic<bool> ran{false};
    const JobHandle h = js.submit([&ran] { ran.store(true); });

    js.wait(h);
    CHECK(ran.load());
}

TEST_CASE("waiting on an invalid handle returns immediately")
{
    JobSystem& js = JobSystem::get();
    JobHandle none;
    js.wait(none); // must not hang
    CHECK_FALSE(none.valid());
}

TEST_CASE("waiting on an already-finished handle returns immediately")
{
    JobSystem& js = JobSystem::get();

    const JobHandle h = js.submit([] {});
    js.wait(h);
    js.wait(h); // stale by now — recycled slots must not hang
    CHECK(true);
}

TEST_CASE("a dependency runs before its dependent")
{
    JobSystem& js = JobSystem::get();

    std::atomic<int> ticket{0};
    int first  = -1;
    int second = -1;

    const JobHandle a = js.submit([&] { first = ticket.fetch_add(1); });

    const std::array<JobHandle, 1> deps{a};
    const JobHandle b = js.submitAfter(deps, [&] { second = ticket.fetch_add(1); });

    js.wait(b);

    CHECK(first == 0);
    CHECK(second == 1);
}

TEST_CASE("a chain of dependencies runs in order")
{
    JobSystem& js = JobSystem::get();

    constexpr int kLength = 32;
    std::vector<int> order(kLength, -1);
    std::atomic<int> ticket{0};

    JobHandle previous;
    for (int i = 0; i < kLength; ++i) {
        auto body = [&order, &ticket, i] { order[static_cast<std::size_t>(i)] = ticket.fetch_add(1); };

        if (i == 0) {
            previous = js.submit(body);
        } else {
            const std::array<JobHandle, 1> deps{previous};
            previous = js.submitAfter(deps, body);
        }
    }

    js.wait(previous);

    for (int i = 0; i < kLength; ++i) {
        CHECK(order[static_cast<std::size_t>(i)] == i);
    }
}

TEST_CASE("fan-in waits for every dependency")
{
    JobSystem& js = JobSystem::get();

    constexpr std::size_t kWidth = 64;
    std::atomic<int> completed{0};

    std::vector<JobHandle> deps;
    deps.reserve(kWidth);
    for (std::size_t i = 0; i < kWidth; ++i) {
        deps.push_back(js.submit([&completed] { completed.fetch_add(1); }));
    }

    int observed = -1;
    const JobHandle join = js.submitAfter(deps, [&] { observed = completed.load(); });

    js.wait(join);

    CHECK(observed == static_cast<int>(kWidth));
    CHECK(completed.load() == static_cast<int>(kWidth));
}

TEST_CASE("many independent jobs all run exactly once")
{
    JobSystem& js = JobSystem::get();

    constexpr std::size_t kCount = 5000;
    std::atomic<int> counter{0};

    std::vector<JobHandle> handles;
    handles.reserve(kCount);
    for (std::size_t i = 0; i < kCount; ++i) {
        handles.push_back(js.submit([&counter] { counter.fetch_add(1); }));
    }
    for (const JobHandle h : handles) {
        js.wait(h);
    }

    CHECK(counter.load() == static_cast<int>(kCount));
}

TEST_CASE("waitIdle drains everything that was submitted")
{
    JobSystem& js = JobSystem::get();

    constexpr std::size_t kCount = 2000;
    std::atomic<int> counter{0};

    for (std::size_t i = 0; i < kCount; ++i) {
        (void)js.submit([&counter] { counter.fetch_add(1); });
    }

    js.waitIdle();
    CHECK(counter.load() == static_cast<int>(kCount));
}

TEST_CASE("parallelFor visits every index exactly once")
{
    JobSystem& js = JobSystem::get();

    constexpr std::size_t kCount = 10000;
    std::vector<int> visits(kCount, 0);

    // Chunks are disjoint, so plain ints are safe here.
    js.parallelFor(kCount, 64, [&visits](std::size_t begin, std::size_t end) {
        for (std::size_t i = begin; i < end; ++i) {
            visits[i] += 1;
        }
    });

    for (const int v : visits) {
        CHECK(v == 1);
    }
}

TEST_CASE("parallelFor computes the same sum as a serial loop")
{
    JobSystem& js = JobSystem::get();

    constexpr std::size_t kCount = 100000;
    std::vector<std::int64_t> data(kCount);
    std::iota(data.begin(), data.end(), 1);

    const std::int64_t expected = std::accumulate(data.begin(), data.end(), std::int64_t{0});

    std::atomic<std::int64_t> total{0};
    js.parallelFor(kCount, 1024, [&](std::size_t begin, std::size_t end) {
        std::int64_t local = 0;
        for (std::size_t i = begin; i < end; ++i) {
            local += data[i];
        }
        total.fetch_add(local);
    });

    CHECK(total.load() == expected);
}

TEST_CASE("parallelFor handles a count smaller than the grain")
{
    JobSystem& js = JobSystem::get();

    std::atomic<int> calls{0};
    std::atomic<int> covered{0};

    js.parallelFor(5, 1024, [&](std::size_t begin, std::size_t end) {
        calls.fetch_add(1);
        covered.fetch_add(static_cast<int>(end - begin));
    });

    CHECK(calls.load() == 1);
    CHECK(covered.load() == 5);
}

TEST_CASE("parallelFor with zero count does nothing")
{
    JobSystem& js = JobSystem::get();

    std::atomic<int> calls{0};
    js.parallelFor(0, 16, [&](std::size_t, std::size_t) { calls.fetch_add(1); });

    CHECK(calls.load() == 0);
}

TEST_CASE("a job can submit and wait on nested work without deadlocking")
{
    JobSystem& js = JobSystem::get();

    std::atomic<int> inner{0};

    const JobHandle outer = js.submit([&] {
        std::vector<JobHandle> children;
        children.reserve(8);
        for (int i = 0; i < 8; ++i) {
            children.push_back(JobSystem::get().submit([&inner] { inner.fetch_add(1); }));
        }
        for (const JobHandle c : children) {
            JobSystem::get().wait(c);
        }
    });

    js.wait(outer);
    CHECK(inner.load() == 8);
}

TEST_CASE("nested parallelFor does not deadlock")
{
    JobSystem& js = JobSystem::get();

    std::atomic<int> total{0};

    js.parallelFor(16, 1, [&](std::size_t, std::size_t) {
        JobSystem::get().parallelFor(16, 1, [&](std::size_t, std::size_t) {
            total.fetch_add(1);
        });
    });

    CHECK(total.load() == 16 * 16);
}

TEST_CASE("stats count what ran")
{
    JobSystem& js = JobSystem::get();

    const JobSystem::Stats before = js.stats();

    constexpr int kCount = 100;
    std::vector<JobHandle> handles;
    handles.reserve(kCount);
    for (int i = 0; i < kCount; ++i) {
        handles.push_back(js.submit([] {}));
    }
    for (const JobHandle h : handles) {
        js.wait(h);
    }

    const JobSystem::Stats after = js.stats();
    CHECK(after.submitted >= before.submitted + kCount);
    CHECK(after.executed >= before.executed + kCount);
}
