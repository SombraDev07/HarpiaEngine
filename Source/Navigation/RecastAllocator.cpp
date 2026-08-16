#include "Navigation/RecastAllocator.h"

#include "Core/Memory/MemoryTracker.h"

#include <RecastAlloc.h>
#include <DetourAlloc.h>

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <mutex>

namespace harpia::nav {
namespace {

struct Header {
    std::size_t totalBytes = 0;
    std::size_t alignment  = 0;
    std::size_t userOffset = 0;
};

[[nodiscard]] void* allocateTagged(std::size_t userBytes, std::size_t alignment)
{
    if (userBytes == 0) {
        return nullptr;
    }

    const std::size_t align = std::max(alignment, alignof(Header));
    const std::size_t total = userBytes + sizeof(Header) + align;
    void* const raw = harpia::allocate(total, align, MemTag::Scene);
    if (raw == nullptr) {
        return nullptr;
    }

    const auto rawAddr = reinterpret_cast<std::uintptr_t>(raw);
    const auto minUser = rawAddr + sizeof(Header);
    const auto userAddr = (minUser + align - 1u) & ~(static_cast<std::uintptr_t>(align) - 1u);

    auto* header = reinterpret_cast<Header*>(userAddr - sizeof(Header));
    header->totalBytes = total;
    header->alignment  = align;
    header->userOffset = userAddr - rawAddr;
    return reinterpret_cast<void*>(userAddr);
}

void freeTagged(void* user) noexcept
{
    if (user == nullptr) {
        return;
    }
    auto* header = reinterpret_cast<Header*>(
        reinterpret_cast<std::uintptr_t>(user) - sizeof(Header));
    void* const raw = reinterpret_cast<void*>(
        reinterpret_cast<std::uintptr_t>(user) - header->userOffset);
    harpia::deallocate(raw, header->totalBytes, header->alignment, MemTag::Scene);
}

void* recastAlloc(std::size_t size, rcAllocHint)
{
    return allocateTagged(size, 16);
}

void recastFree(void* ptr)
{
    freeTagged(ptr);
}

void* detourAlloc(std::size_t size, dtAllocHint)
{
    return allocateTagged(size, 16);
}

void detourFree(void* ptr)
{
    freeTagged(ptr);
}

} // namespace

void installRecastAllocator()
{
    static std::once_flag once;
    std::call_once(once, [] {
        rcAllocSetCustom(recastAlloc, recastFree);
        dtAllocSetCustom(detourAlloc, detourFree);
    });
}

} // namespace harpia::nav
