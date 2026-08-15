// Harpia Engine — typed pool with generational handles
//
// Roadmap 1.2 / 1.5: long-lived objects live here and are referenced by handle,
// never by raw pointer. The generation counter makes a stale handle detectable
// instead of a use-after-free.
#pragma once

#include "Core/Memory/MemoryTracker.h"

#include <cstddef>
#include <cstdint>
#include <memory>
#include <new>
#include <type_traits>
#include <utility>

namespace harpia {

// Handles are typed so a TextureHandle cannot be passed where a BufferHandle
// is expected. generation == 0 is always invalid.
template <typename T>
struct Handle {
    std::uint32_t index      = 0;
    std::uint32_t generation = 0;

    [[nodiscard]] constexpr bool valid() const noexcept { return generation != 0; }

    [[nodiscard]] friend constexpr bool operator==(Handle a, Handle b) noexcept
    {
        return a.index == b.index && a.generation == b.generation;
    }
};

// Fixed-capacity pool. A slot's generation is odd while alive, even while free,
// so no separate liveness bitmap is needed.
template <typename T>
class Pool {
public:
    using HandleType = Handle<T>;

    Pool() = default;

    Pool(std::uint32_t capacity, MemTag tag)
        : capacity_(capacity)
        , tag_(tag)
    {
        if (capacity_ == 0) {
            return;
        }

        storage_ = static_cast<std::byte*>(
            harpia::allocate(sizeof(T) * capacity_, alignof(T), tag_));
        generations_ = static_cast<std::uint32_t*>(
            harpia::allocate(sizeof(std::uint32_t) * capacity_, alignof(std::uint32_t), tag_));
        nextFree_ = static_cast<std::uint32_t*>(
            harpia::allocate(sizeof(std::uint32_t) * capacity_, alignof(std::uint32_t), tag_));

        for (std::uint32_t i = 0; i < capacity_; ++i) {
            generations_[i] = 0;
            nextFree_[i]    = i + 1; // capacity_ marks end of list
        }
        firstFree_ = 0;
    }

    ~Pool() { release(); }

    Pool(const Pool&)            = delete;
    Pool& operator=(const Pool&) = delete;

    Pool(Pool&& other) noexcept { moveFrom(other); }

    Pool& operator=(Pool&& other) noexcept
    {
        if (this != &other) {
            release();
            moveFrom(other);
        }
        return *this;
    }

    // Returns an invalid handle when the pool is full.
    template <typename... Args>
    [[nodiscard]] HandleType create(Args&&... args)
    {
        if (firstFree_ >= capacity_) {
            return {};
        }

        const std::uint32_t index = firstFree_;
        firstFree_ = nextFree_[index];

        ::new (slot(index)) T(std::forward<Args>(args)...);

        generations_[index] += 1; // even -> odd: now alive
        ++aliveCount_;

        return HandleType{index, generations_[index]};
    }

    [[nodiscard]] T* get(HandleType handle) noexcept
    {
        return isLive(handle) ? slot(handle.index) : nullptr;
    }

    [[nodiscard]] const T* get(HandleType handle) const noexcept
    {
        return isLive(handle) ? slot(handle.index) : nullptr;
    }

    // Returns false when the handle was already stale.
    bool destroy(HandleType handle) noexcept
    {
        if (!isLive(handle)) {
            return false;
        }

        slot(handle.index)->~T();

        generations_[handle.index] += 1; // odd -> even: now free
        nextFree_[handle.index] = firstFree_;
        firstFree_              = handle.index;
        --aliveCount_;
        return true;
    }

    void clear() noexcept
    {
        for (std::uint32_t i = 0; i < capacity_; ++i) {
            if ((generations_[i] & 1u) != 0u) {
                slot(i)->~T();
                generations_[i] += 1;
            }
            nextFree_[i] = i + 1;
        }
        firstFree_  = capacity_ > 0 ? 0 : 0;
        aliveCount_ = 0;
    }

    [[nodiscard]] std::uint32_t size() const noexcept     { return aliveCount_; }
    [[nodiscard]] std::uint32_t capacity() const noexcept { return capacity_; }
    [[nodiscard]] bool          empty() const noexcept    { return aliveCount_ == 0; }
    [[nodiscard]] bool          full() const noexcept     { return firstFree_ >= capacity_; }

private:
    [[nodiscard]] bool isLive(HandleType handle) const noexcept
    {
        return handle.valid()
            && handle.index < capacity_
            && generations_[handle.index] == handle.generation;
    }

    [[nodiscard]] T* slot(std::uint32_t index) noexcept
    {
        return std::launder(reinterpret_cast<T*>(storage_ + sizeof(T) * index));
    }

    [[nodiscard]] const T* slot(std::uint32_t index) const noexcept
    {
        return std::launder(reinterpret_cast<const T*>(storage_ + sizeof(T) * index));
    }

    void release() noexcept
    {
        if (storage_ != nullptr) {
            for (std::uint32_t i = 0; i < capacity_; ++i) {
                if ((generations_[i] & 1u) != 0u) {
                    slot(i)->~T();
                }
            }
            harpia::deallocate(storage_, sizeof(T) * capacity_, alignof(T), tag_);
            harpia::deallocate(generations_, sizeof(std::uint32_t) * capacity_,
                               alignof(std::uint32_t), tag_);
            harpia::deallocate(nextFree_, sizeof(std::uint32_t) * capacity_,
                               alignof(std::uint32_t), tag_);
        }
        storage_     = nullptr;
        generations_ = nullptr;
        nextFree_    = nullptr;
        capacity_    = 0;
        aliveCount_  = 0;
        firstFree_   = 0;
    }

    void moveFrom(Pool& other) noexcept
    {
        storage_     = other.storage_;
        generations_ = other.generations_;
        nextFree_    = other.nextFree_;
        capacity_    = other.capacity_;
        aliveCount_  = other.aliveCount_;
        firstFree_   = other.firstFree_;
        tag_         = other.tag_;

        other.storage_     = nullptr;
        other.generations_ = nullptr;
        other.nextFree_    = nullptr;
        other.capacity_    = 0;
        other.aliveCount_  = 0;
        other.firstFree_   = 0;
    }

    std::byte*     storage_     = nullptr;
    std::uint32_t* generations_ = nullptr;
    std::uint32_t* nextFree_    = nullptr;
    std::uint32_t  capacity_    = 0;
    std::uint32_t  aliveCount_  = 0;
    std::uint32_t  firstFree_   = 0;
    MemTag         tag_         = MemTag::General;
};

} // namespace harpia
