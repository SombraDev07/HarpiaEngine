// Golden-image comparison helpers.
//
// Roadmap rule 1: every phase ends in a verifiable image. This is the tooling
// that makes "verifiable" mean a number rather than an opinion.
#pragma once

#include <cmath>
#include <cstddef>
#include <cstdint>
#include <span>

namespace harpia::test {

struct ImageDiff {
    std::size_t differingPixels = 0;
    int         maxChannelDelta = 0;
    double      meanAbsError    = 0.0;
    bool        sizeMismatch    = false;
};

// Both buffers are tightly packed RGBA8.
[[nodiscard]] inline ImageDiff compareRgba(std::span<const std::uint8_t> a,
                                           std::span<const std::uint8_t> b,
                                           int                           tolerance = 0)
{
    ImageDiff diff;
    if (a.size() != b.size() || a.empty()) {
        diff.sizeMismatch = true;
        return diff;
    }

    std::uint64_t absErrorSum = 0;

    for (std::size_t pixel = 0; pixel + 3 < a.size(); pixel += 4) {
        bool differs = false;
        for (std::size_t channel = 0; channel < 4; ++channel) {
            const int delta = std::abs(static_cast<int>(a[pixel + channel])
                                     - static_cast<int>(b[pixel + channel]));
            absErrorSum += static_cast<std::uint64_t>(delta);
            if (delta > diff.maxChannelDelta) {
                diff.maxChannelDelta = delta;
            }
            if (delta > tolerance) {
                differs = true;
            }
        }
        if (differs) {
            ++diff.differingPixels;
        }
    }

    diff.meanAbsError = static_cast<double>(absErrorSum) / static_cast<double>(a.size());
    return diff;
}

} // namespace harpia::test
