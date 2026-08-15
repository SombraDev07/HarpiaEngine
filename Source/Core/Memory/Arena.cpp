#include "Core/Memory/Arena.h"

namespace harpia {
namespace {

constexpr std::size_t kBlockAlignment = 64; // cache line

[[nodiscard]] constexpr std::size_t alignUp(std::size_t value, std::size_t alignment) noexcept
{
    return (value + alignment - 1) & ~(alignment - 1);
}

} // namespace

Arena::Arena(std::size_t capacity, MemTag tag)
    : capacity_(alignUp(capacity, kBlockAlignment))
    , tag_(tag)
{
    if (capacity_ > 0) {
        base_ = static_cast<std::uint8_t*>(
            harpia::allocate(capacity_, kBlockAlignment, tag_));
    }
}

Arena::~Arena()
{
    release();
}

Arena::Arena(Arena&& other) noexcept
    : base_(other.base_)
    , capacity_(other.capacity_)
    , offset_(other.offset_)
    , highWater_(other.highWater_)
    , tag_(other.tag_)
{
    other.base_      = nullptr;
    other.capacity_  = 0;
    other.offset_    = 0;
    other.highWater_ = 0;
}

Arena& Arena::operator=(Arena&& other) noexcept
{
    if (this != &other) {
        release();

        base_      = other.base_;
        capacity_  = other.capacity_;
        offset_    = other.offset_;
        highWater_ = other.highWater_;
        tag_       = other.tag_;

        other.base_      = nullptr;
        other.capacity_  = 0;
        other.offset_    = 0;
        other.highWater_ = 0;
    }
    return *this;
}

void Arena::release() noexcept
{
    if (base_ != nullptr) {
        harpia::deallocate(base_, capacity_, kBlockAlignment, tag_);
        base_ = nullptr;
    }
    capacity_ = 0;
    offset_   = 0;
}

void* Arena::allocate(std::size_t bytes, std::size_t alignment) noexcept
{
    if (bytes == 0 || base_ == nullptr) {
        return nullptr;
    }
    if (alignment == 0 || (alignment & (alignment - 1)) != 0) {
        return nullptr; // alignment must be a power of two
    }

    // Align the absolute address, not the offset: the block itself is only
    // guaranteed to be kBlockAlignment-aligned, so aligning the offset alone
    // would hand back a misaligned pointer for any stricter request.
    const auto baseAddress    = reinterpret_cast<std::uintptr_t>(base_);
    const std::uintptr_t mask = static_cast<std::uintptr_t>(alignment) - 1;
    const std::uintptr_t alignedAddress = (baseAddress + offset_ + mask) & ~mask;
    const std::size_t    aligned = static_cast<std::size_t>(alignedAddress - baseAddress);

    // Checked this way round so a huge `bytes` cannot wrap the addition.
    if (aligned > capacity_ || bytes > capacity_ - aligned) {
        return nullptr;
    }

    std::uint8_t* result = base_ + aligned;
    offset_ = aligned + bytes;

    if (offset_ > highWater_) {
        highWater_ = offset_;
    }
    return result;
}

void Arena::reset() noexcept
{
    offset_ = 0;
}

} // namespace harpia
