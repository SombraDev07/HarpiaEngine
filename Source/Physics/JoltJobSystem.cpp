#include "Physics/JoltJobSystem.h"

#include "Core/Threading/JobSystem.h"

#include <thread>

namespace harpia::phys {

JoltJobSystem::JoltJobSystem(JPH::uint maxJobs, JPH::uint maxBarriers)
{
    JobSystemWithBarrier::Init(maxBarriers);
    jobs_.Init(maxJobs, maxJobs);
}

int JoltJobSystem::GetMaxConcurrency() const
{
    const harpia::JobSystem& js = harpia::JobSystem::get();
    if (!js.initialized()) {
        return 1;
    }
    // The waiting thread participates: JobSystemWithBarrier::WaitForJobs
    // executes barrier jobs on the caller, same as Jolt's own thread pool.
    return static_cast<int>(js.workerCount()) + 1;
}

JoltJobSystem::JobHandle JoltJobSystem::CreateJob(const char*        name,
                                                  JPH::ColorArg      color,
                                                  const JobFunction& function,
                                                  JPH::uint32        numDependencies)
{
    JPH::uint32 index = JPH::FixedSizeFreeList<Job>::cInvalidObjectIndex;
    for (;;) {
        index = jobs_.ConstructObject(name, color, this, function, numDependencies);
        if (index != JPH::FixedSizeFreeList<Job>::cInvalidObjectIndex) {
            break;
        }
        // The pool is finite; spinning here matches Jolt's own thread pool.
        std::this_thread::yield();
    }

    Job* job = &jobs_.Get(index);
    JobHandle handle(job);
    if (numDependencies == 0) {
        QueueJob(job);
    }
    return handle;
}

void JoltJobSystem::QueueJob(Job* job)
{
    harpia::JobSystem& js = harpia::JobSystem::get();
    if (!js.initialized()) {
        // No workers: JobSystemWithBarrier::WaitForJobs runs the job itself.
        return;
    }

    job->AddRef();
    js.submit([job] {
        job->Execute();
        job->Release();
    }, "jolt");
}

void JoltJobSystem::QueueJobs(Job** jobs, JPH::uint count)
{
    for (JPH::uint i = 0; i < count; ++i) {
        QueueJob(jobs[i]);
    }
}

void JoltJobSystem::FreeJob(Job* job)
{
    jobs_.DestructObject(job);
}

} // namespace harpia::phys
