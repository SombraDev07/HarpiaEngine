// Harpia Engine — job system with dependency graph and work stealing
//
// Roadmap 1.1: task-based, not fibers. Fibers scale slightly better and cost
// you Tracy, gdb and every crash investigation. This stops at column 3 of the
// ladder on purpose.
//
// Rule: no subsystem spawns its own thread. Everything goes through here.
#pragma once

#include <cstddef>
#include <cstdint>
#include <functional>
#include <span>

namespace harpia {

struct JobHandle {
    std::uint32_t index      = 0;
    std::uint32_t generation = 0;

    [[nodiscard]] constexpr bool valid() const noexcept { return generation != 0; }
};

class JobSystem {
public:
    static JobSystem& get();

    // workerCount == 0 means hardware_concurrency() - 1 (the calling thread
    // participates, so we do not oversubscribe).
    void init(std::uint32_t workerCount = 0);
    void shutdown();

    [[nodiscard]] bool          initialized() const noexcept;
    [[nodiscard]] std::uint32_t workerCount() const noexcept;

    // Schedules immediately.
    JobHandle submit(std::function<void()> fn, const char* name = "job");

    // Schedules once every dependency has finished. Dependencies that are
    // already complete (or stale) count as satisfied.
    JobHandle submitAfter(std::span<const JobHandle> deps,
                          std::function<void()>      fn,
                          const char*                name = "job");

    // Blocks until the job finishes. The calling thread executes pending work
    // while it waits, so waiting from inside a job cannot deadlock.
    void wait(JobHandle handle);

    // Blocks until no job is queued or running.
    void waitIdle();

    // Splits [0, count) into chunks of at most `grain` and runs them in
    // parallel. Returns once every chunk has finished.
    void parallelFor(std::size_t                                          count,
                     std::size_t                                          grain,
                     const std::function<void(std::size_t, std::size_t)>& fn,
                     const char*                                          name = "parallel_for");

    // Index of the calling worker, or kNotAWorker for any other thread.
    static constexpr std::uint32_t kNotAWorker = 0xFFFFFFFFu;
    [[nodiscard]] static std::uint32_t currentWorker() noexcept;

    struct Stats {
        std::uint64_t submitted = 0;
        std::uint64_t executed  = 0;
        std::uint64_t stolen    = 0;
    };
    [[nodiscard]] Stats stats() const noexcept;

private:
    JobSystem();
    ~JobSystem();

    JobSystem(const JobSystem&)            = delete;
    JobSystem& operator=(const JobSystem&) = delete;

    struct Impl;
    Impl* impl_;
};

} // namespace harpia
