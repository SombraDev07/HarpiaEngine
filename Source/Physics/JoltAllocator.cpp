#include "Physics/JoltAllocator.h"

#include "Core/Memory/MemoryTracker.h"

#include <Jolt/Jolt.h>
#include <Jolt/Core/Memory.h>

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <mutex>

namespace harpia::phys {
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

    void* const raw = harpia::allocate(total, align, MemTag::Physics);
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
    harpia::deallocate(raw, header->totalBytes, header->alignment, MemTag::Physics);
}

[[nodiscard]] Header* headerOf(void* user) noexcept
{
    return reinterpret_cast<Header*>(
        reinterpret_cast<std::uintptr_t>(user) - sizeof(Header));
}

void* joltAllocate(std::size_t size)
{
    return allocateTagged(size, 16);
}

void* joltReallocate(void* block, std::size_t oldSize, std::size_t newSize)
{
    if (block == nullptr) {
        return allocateTagged(newSize, 16);
    }
    if (newSize == 0) {
        freeTagged(block);
        return nullptr;
    }

    void* const fresh = allocateTagged(newSize, 16);
    if (fresh == nullptr) {
        return nullptr;
    }

    const std::size_t knownOld = headerOf(block)->totalBytes - headerOf(block)->userOffset;
    const std::size_t toCopy = std::min({oldSize, knownOld, newSize});
    if (toCopy > 0) {
        std::memcpy(fresh, block, toCopy);
    }
    freeTagged(block);
    return fresh;
}

void joltFree(void* block)
{
    freeTagged(block);
}

void* joltAlignedAllocate(std::size_t size, std::size_t alignment)
{
    return allocateTagged(size, alignment);
}

void joltAlignedFree(void* block)
{
    freeTagged(block);
}

} // namespace

void installJoltAllocator()
{
    static std::once_flag once;
    std::call_once(once, [] {
        JPH::Allocate        = joltAllocate;
        JPH::Reallocate      = joltReallocate;
        JPH::Free            = joltFree;
        JPH::AlignedAllocate = joltAlignedAllocate;
        JPH::AlignedFree     = joltAlignedFree;
    });
}

} // namespace harpia::phys
