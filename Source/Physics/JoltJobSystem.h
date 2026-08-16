// Harpia Engine — Jolt jobs run on the engine JobSystem
//
// Invariant 2: no subsystem creates a thread. Jolt's JobSystemThreadPool
// would. This implementation only queues; Harpia workers (and the waiting
// thread, via JobSystemWithBarrier) execute.
#pragma once

#include <Jolt/Jolt.h>
#include <Jolt/Core/FixedSizeFreeList.h>
#include <Jolt/Core/JobSystemWithBarrier.h>

namespace harpia::phys {

class JoltJobSystem final : public JPH::JobSystemWithBarrier {
public:
    JoltJobSystem(JPH::uint maxJobs, JPH::uint maxBarriers);
    ~JoltJobSystem() override = default;

    int GetMaxConcurrency() const override;

    JobHandle CreateJob(const char*          name,
                        JPH::ColorArg        color,
                        const JobFunction&   function,
                        JPH::uint32          numDependencies = 0) override;

protected:
    void QueueJob(Job* job) override;
    void QueueJobs(Job** jobs, JPH::uint count) override;
    void FreeJob(Job* job) override;

private:
    JPH::FixedSizeFreeList<Job> jobs_;
};

} // namespace harpia::phys
