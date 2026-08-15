#include "Core/Threading/JobSystem.h"

#include "Core/Profiling/Profiler.h"

#include <algorithm>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstdio>
#include <deque>
#include <memory>
#include <mutex>
#include <random>
#include <thread>
#include <vector>

namespace harpia {
namespace {

// Worker index of the calling thread. kNotAWorker on the main thread and on any
// thread the engine did not create.
thread_local std::uint32_t tlsWorkerIndex = JobSystem::kNotAWorker;

// A job slot is allocated while its generation is odd, free while even.
// A handle whose generation no longer matches refers to a job that has
// definitely completed — that is what makes stale handles safe to wait on.
struct JobEntry {
    std::function<void()>      fn;
    std::vector<std::uint32_t> dependents;
    std::uint32_t              depsRemaining = 0;
    std::uint32_t              generation    = 0;
    const char*                name          = "job";
};

struct WorkerQueue {
    std::mutex                m;
    std::deque<std::uint32_t> q;
};

} // namespace

struct JobSystem::Impl {
    // --- job graph (guarded by graphMutex) ---------------------------------
    std::mutex                              graphMutex;
    std::vector<std::unique_ptr<JobEntry>>  jobs;
    std::vector<std::uint32_t>              freeJobs;

    // --- ready queues ------------------------------------------------------
    std::vector<std::unique_ptr<WorkerQueue>> queues;
    std::atomic<std::uint32_t>                pushCursor{0};
    std::atomic<std::uint32_t>                readyCount{0};

    // --- workers -----------------------------------------------------------
    std::vector<std::thread>  threads;
    std::atomic<bool>         running{false};
    std::atomic<std::uint64_t> inFlight{0};

    std::mutex              sleepMutex;
    std::condition_variable sleepCv;

    // --- stats -------------------------------------------------------------
    std::atomic<std::uint64_t> submitted{0};
    std::atomic<std::uint64_t> executed{0};
    std::atomic<std::uint64_t> stolen{0};

    [[nodiscard]] std::uint32_t queueCount() const noexcept
    {
        return static_cast<std::uint32_t>(queues.size());
    }

    void enqueue(std::uint32_t jobIndex)
    {
        const std::uint32_t count = queueCount();
        if (count == 0) {
            return;
        }

        std::uint32_t target = tlsWorkerIndex;
        if (target >= count) {
            target = pushCursor.fetch_add(1, std::memory_order_relaxed) % count;
        }

        {
            WorkerQueue& wq = *queues[target];
            std::lock_guard<std::mutex> lk(wq.m);
            wq.q.push_back(jobIndex);
        }

        readyCount.fetch_add(1, std::memory_order_release);
        sleepCv.notify_one();
    }

    // Takes from the caller's own queue first (LIFO, warm cache), then steals
    // from the front of another worker's queue (oldest work, least contended).
    [[nodiscard]] bool tryPop(std::uint32_t worker, std::uint32_t& outJob)
    {
        const std::uint32_t count = queueCount();
        if (count == 0) {
            return false;
        }

        if (worker < count) {
            WorkerQueue& wq = *queues[worker];
            std::lock_guard<std::mutex> lk(wq.m);
            if (!wq.q.empty()) {
                outJob = wq.q.back();
                wq.q.pop_back();
                readyCount.fetch_sub(1, std::memory_order_acq_rel);
                return true;
            }
        }

        // Start stealing from a pseudo-random victim so N workers that go idle
        // together do not all hammer queue 0.
        static thread_local std::minstd_rand rng{std::random_device{}()};
        const std::uint32_t start = static_cast<std::uint32_t>(rng() % count);

        for (std::uint32_t i = 0; i < count; ++i) {
            const std::uint32_t victim = (start + i) % count;
            if (victim == worker) {
                continue;
            }

            WorkerQueue& wq = *queues[victim];
            std::lock_guard<std::mutex> lk(wq.m);
            if (!wq.q.empty()) {
                outJob = wq.q.front();
                wq.q.pop_front();
                readyCount.fetch_sub(1, std::memory_order_acq_rel);
                stolen.fetch_add(1, std::memory_order_relaxed);
                return true;
            }
        }
        return false;
    }

    // Runs one job if any is available. Returns false when there was nothing
    // to do, which is the caller's cue to sleep or yield.
    bool tryExecuteOne(std::uint32_t worker)
    {
        std::uint32_t jobIndex = 0;
        if (!tryPop(worker, jobIndex)) {
            return false;
        }

        std::function<void()> fn;
        const char*           name = "job";
        {
            std::lock_guard<std::mutex> lk(graphMutex);
            JobEntry& job = *jobs[jobIndex];
            fn   = std::move(job.fn);
            name = job.name;
        }

        if (fn) {
            HARPIA_ZONE_NAMED("job");
            (void)name;
            fn();
        }

        // Destroy the callable (and whatever it captured) before touching the
        // graph again — an arbitrary destructor should not run under our lock.
        fn = nullptr;

        complete(jobIndex);
        executed.fetch_add(1, std::memory_order_relaxed);
        return true;
    }

    // Retires a finished job: recycles its slot, then releases any dependent
    // whose last dependency this was.
    void complete(std::uint32_t jobIndex)
    {
        std::vector<std::uint32_t> nowReady;
        {
            std::lock_guard<std::mutex> lk(graphMutex);
            JobEntry& job = *jobs[jobIndex];

            for (const std::uint32_t dependent : job.dependents) {
                JobEntry& other = *jobs[dependent];
                if (other.depsRemaining > 0 && --other.depsRemaining == 0) {
                    nowReady.push_back(dependent);
                }
            }
            job.dependents.clear();

            job.generation += 1; // odd -> even: slot is free, handle goes stale
            freeJobs.push_back(jobIndex);
        }

        for (const std::uint32_t ready : nowReady) {
            enqueue(ready);
        }

        inFlight.fetch_sub(1, std::memory_order_acq_rel);
    }

    [[nodiscard]] bool isFinished(JobHandle handle)
    {
        if (!handle.valid()) {
            return true;
        }
        std::lock_guard<std::mutex> lk(graphMutex);
        if (handle.index >= jobs.size()) {
            return true;
        }
        return jobs[handle.index]->generation != handle.generation;
    }

    void workerMain(std::uint32_t workerIndex)
    {
        tlsWorkerIndex = workerIndex;

        char threadName[32];
        std::snprintf(threadName, sizeof(threadName), "harpia-worker-%u", workerIndex);
        HARPIA_THREAD_NAME(threadName);

        while (running.load(std::memory_order_acquire)) {
            if (tryExecuteOne(workerIndex)) {
                continue;
            }

            // The timeout makes a missed notify a 1 ms latency blip instead of
            // a hang, which keeps the wakeup path simple enough to reason about.
            std::unique_lock<std::mutex> lk(sleepMutex);
            sleepCv.wait_for(lk, std::chrono::milliseconds(1), [this] {
                return !running.load(std::memory_order_acquire)
                    || readyCount.load(std::memory_order_acquire) > 0;
            });
        }

        tlsWorkerIndex = JobSystem::kNotAWorker;
    }
};

// ---------------------------------------------------------------------------

JobSystem::JobSystem()
    : impl_(new Impl())
{
}

JobSystem::~JobSystem()
{
    shutdown();
    delete impl_;
}

JobSystem& JobSystem::get()
{
    static JobSystem instance;
    return instance;
}

void JobSystem::init(std::uint32_t workerCount)
{
    if (impl_->running.load(std::memory_order_acquire)) {
        return;
    }

    if (workerCount == 0) {
        const unsigned hw = std::thread::hardware_concurrency();
        workerCount = hw > 1 ? hw - 1 : 1;
    }

    impl_->queues.clear();
    impl_->queues.reserve(workerCount);
    for (std::uint32_t i = 0; i < workerCount; ++i) {
        impl_->queues.push_back(std::make_unique<WorkerQueue>());
    }

    impl_->running.store(true, std::memory_order_release);

    impl_->threads.reserve(workerCount);
    for (std::uint32_t i = 0; i < workerCount; ++i) {
        impl_->threads.emplace_back([this, i] { impl_->workerMain(i); });
    }
}

void JobSystem::shutdown()
{
    if (!impl_->running.load(std::memory_order_acquire)) {
        return;
    }

    waitIdle();

    impl_->running.store(false, std::memory_order_release);
    impl_->sleepCv.notify_all();

    for (std::thread& t : impl_->threads) {
        if (t.joinable()) {
            t.join();
        }
    }
    impl_->threads.clear();
    impl_->queues.clear();

    {
        std::lock_guard<std::mutex> lk(impl_->graphMutex);
        impl_->jobs.clear();
        impl_->freeJobs.clear();
    }
    impl_->readyCount.store(0, std::memory_order_release);
}

bool JobSystem::initialized() const noexcept
{
    return impl_->running.load(std::memory_order_acquire);
}

std::uint32_t JobSystem::workerCount() const noexcept
{
    return impl_->queueCount();
}

std::uint32_t JobSystem::currentWorker() noexcept
{
    return tlsWorkerIndex;
}

JobHandle JobSystem::submit(std::function<void()> fn, const char* name)
{
    return submitAfter({}, std::move(fn), name);
}

JobHandle JobSystem::submitAfter(std::span<const JobHandle> deps,
                                 std::function<void()>      fn,
                                 const char*                name)
{
    // Without workers there is nowhere to defer to; run inline so single-thread
    // builds and tests that never call init() still behave correctly.
    if (!impl_->running.load(std::memory_order_acquire)) {
        if (fn) {
            fn();
        }
        impl_->submitted.fetch_add(1, std::memory_order_relaxed);
        impl_->executed.fetch_add(1, std::memory_order_relaxed);
        return {};
    }

    std::uint32_t jobIndex = 0;
    bool          ready    = false;
    JobHandle     handle{};

    {
        std::lock_guard<std::mutex> lk(impl_->graphMutex);

        if (!impl_->freeJobs.empty()) {
            jobIndex = impl_->freeJobs.back();
            impl_->freeJobs.pop_back();
        } else {
            impl_->jobs.push_back(std::make_unique<JobEntry>());
            jobIndex = static_cast<std::uint32_t>(impl_->jobs.size() - 1);
        }

        JobEntry& job = *impl_->jobs[jobIndex];
        job.fn            = std::move(fn);
        job.name          = name != nullptr ? name : "job";
        job.depsRemaining = 0;
        job.dependents.clear();
        job.generation += 1; // even -> odd: slot is live

        handle = JobHandle{jobIndex, job.generation};

        for (const JobHandle dep : deps) {
            if (!dep.valid() || dep.index >= impl_->jobs.size()) {
                continue; // never existed
            }
            JobEntry& depJob = *impl_->jobs[dep.index];
            if (depJob.generation != dep.generation) {
                continue; // already finished and recycled
            }
            depJob.dependents.push_back(jobIndex);
            ++job.depsRemaining;
        }

        ready = (job.depsRemaining == 0);
    }

    impl_->inFlight.fetch_add(1, std::memory_order_acq_rel);
    impl_->submitted.fetch_add(1, std::memory_order_relaxed);

    if (ready) {
        impl_->enqueue(jobIndex);
    }
    return handle;
}

void JobSystem::wait(JobHandle handle)
{
    if (!handle.valid()) {
        return;
    }

    while (!impl_->isFinished(handle)) {
        // Help instead of blocking: waiting from inside a job must not deadlock.
        if (!impl_->tryExecuteOne(tlsWorkerIndex)) {
            std::this_thread::yield();
        }
    }
}

void JobSystem::waitIdle()
{
    while (impl_->inFlight.load(std::memory_order_acquire) > 0) {
        if (!impl_->tryExecuteOne(tlsWorkerIndex)) {
            std::this_thread::yield();
        }
    }
}

void JobSystem::parallelFor(std::size_t                                          count,
                            std::size_t                                          grain,
                            const std::function<void(std::size_t, std::size_t)>& fn,
                            const char*                                          name)
{
    if (count == 0 || !fn) {
        return;
    }

    grain = std::max<std::size_t>(grain, 1);
    const std::size_t chunks = (count + grain - 1) / grain;

    if (chunks == 1 || !impl_->running.load(std::memory_order_acquire)) {
        fn(0, count);
        return;
    }

    std::vector<JobHandle> handles;
    handles.reserve(chunks);

    for (std::size_t chunk = 0; chunk < chunks; ++chunk) {
        const std::size_t begin = chunk * grain;
        const std::size_t end   = std::min(begin + grain, count);
        // `fn` outlives the jobs because we wait for all of them below.
        handles.push_back(submit([&fn, begin, end] { fn(begin, end); }, name));
    }

    for (const JobHandle handle : handles) {
        wait(handle);
    }
}

JobSystem::Stats JobSystem::stats() const noexcept
{
    Stats s;
    s.submitted = impl_->submitted.load(std::memory_order_relaxed);
    s.executed  = impl_->executed.load(std::memory_order_relaxed);
    s.stolen    = impl_->stolen.load(std::memory_order_relaxed);
    return s;
}

} // namespace harpia
