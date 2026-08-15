// Harpia Engine — frame arena (bump allocator)
//
// Roadmap 1.2: everything transient lives here and dies at reset().
// There is no individual free. Fixed capacity on purpose — an arena that grows
// silently hides the budget it was created to expose.
#pragma once

#include "Core/Memory/MemoryTracker.h"

#include <cstddef>
#include <cstdint>
#include <new>
#include <span>
#include <type_traits>
#include <utility>

namespace harpia {

class Arena {
public:
    Arena() = default;
    Arena(std::size_t capacity, MemTag tag);
    ~Arena();

    Arena(const Arena&)            = delete;
    Arena& operator=(const Arena&) = delete;
    Arena(Arena&& other) noexcept;
    Arena& operator=(Arena&& other) noexcept;

    // Returns nullptr when the arena is exhausted. Callers in the frame path
    // are expected to size the arena from the high-water mark, not to retry.
    [[nodiscard]] void* allocate(std::size_t bytes,
                                 std::size_t alignment = alignof(std::max_align_t)) noexcept;

    // Arenas never run destructors, so only trivially destructible types are
    // allowed. Anything owning memory belongs in a Pool.
    template <typename T, typename... Args>
    [[nodiscard]] T* create(Args&&... args) noexcept
    {
        static_assert(std::is_trivially_destructible_v<T>,
                      "Arena never runs destructors; use Pool for types that own resources");
        void* mem = allocate(sizeof(T), alignof(T));
        if (mem == nullptr) {
            return nullptr;
        }
        return new (mem) T(std::forward<Args>(args)...);
    }

    template <typename T>
    [[nodiscard]] std::span<T> createArray(std::size_t count) noexcept
    {
        static_assert(std::is_trivially_destructible_v<T>,
                      "Arena never runs destructors; use Pool for types that own resources");
        if (count == 0) {
            return {};
        }
        void* mem = allocate(sizeof(T) * count, alignof(T));
        if (mem == nullptr) {
            return {};
        }
        T* array = new (mem) T[count]{};
        return std::span<T>{array, count};
    }

    // Frees everything. Keeps the backing block and the high-water mark.
    void reset() noexcept;

    [[nodiscard]] std::size_t used() const noexcept          { return offset_; }
    [[nodiscard]] std::size_t capacity() const noexcept      { return capacity_; }
    [[nodiscard]] std::size_t remaining() const noexcept     { return capacity_ - offset_; }
    [[nodiscard]] std::size_t highWaterMark() const noexcept { return highWater_; }
    [[nodiscard]] MemTag      tag() const noexcept           { return tag_; }

    // RAII sub-scope: restores the arena offset on destruction. Nests freely.
    class Scope {
    public:
        explicit Scope(Arena& arena) noexcept : arena_(&arena), mark_(arena.offset_) {}
        ~Scope() noexcept { arena_->offset_ = mark_; }

        Scope(const Scope&)            = delete;
        Scope& operator=(const Scope&) = delete;
        Scope(Scope&&)                 = delete;
        Scope& operator=(Scope&&)      = delete;

    private:
        Arena*      arena_;
        std::size_t mark_;
    };

private:
    void release() noexcept;

    std::uint8_t* base_      = nullptr;
    std::size_t   capacity_  = 0;
    std::size_t   offset_    = 0;
    std::size_t   highWater_ = 0;
    MemTag        tag_       = MemTag::Scratch;
};

} // namespace harpia
