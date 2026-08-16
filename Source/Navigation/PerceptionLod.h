// Harpia Engine — perception LOD
//
// Distant agents must think less. The tiers are the policy; the caller
// decides what "think" costs. Without this, every AI ticks at full rate
// and the crowd dies at a few dozen agents.
#pragma once

#include <cstdint>

namespace harpia {

enum class PerceptionTier : std::uint8_t {
    Full    = 0,
    Reduced = 1,
    Sleep   = 2,
};

struct PerceptionLod {
    float         fullRadius     = 20.0f;
    float         reducedRadius  = 60.0f;
    std::uint32_t reducedEvery   = 4;
    std::uint32_t sleepEvery     = 16;

    [[nodiscard]] PerceptionTier classify(float distance) const noexcept
    {
        if (distance <= fullRadius) {
            return PerceptionTier::Full;
        }
        if (distance <= reducedRadius) {
            return PerceptionTier::Reduced;
        }
        return PerceptionTier::Sleep;
    }

    // True when this agent should run its expensive think on `tick`.
    [[nodiscard]] bool shouldThink(PerceptionTier tier, std::uint32_t tick) const noexcept
    {
        switch (tier) {
            case PerceptionTier::Full:    return true;
            case PerceptionTier::Reduced: return reducedEvery == 0 || (tick % reducedEvery) == 0;
            case PerceptionTier::Sleep:   return sleepEvery == 0 || (tick % sleepEvery) == 0;
        }
        return true;
    }
};

} // namespace harpia
