// Harpia Engine — byte stream primitives
//
// Little-endian, which every platform we target is. A big-endian port would
// swap here and nowhere else.
#pragma once

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <span>
#include <string>
#include <string_view>
#include <type_traits>
#include <vector>

namespace harpia::serial {

class ByteWriter {
public:
    template <typename T>
    void writeRaw(const T& value)
    {
        static_assert(std::is_trivially_copyable_v<T>, "writeRaw needs a trivially copyable type");
        const auto* bytes = reinterpret_cast<const std::uint8_t*>(&value);
        buffer_.insert(buffer_.end(), bytes, bytes + sizeof(T));
    }

    void writeBytes(const void* data, std::size_t size)
    {
        if (size == 0) {
            return;
        }
        const auto* bytes = static_cast<const std::uint8_t*>(data);
        buffer_.insert(buffer_.end(), bytes, bytes + size);
    }

    void writeString(std::string_view text)
    {
        writeRaw(static_cast<std::uint32_t>(text.size()));
        writeBytes(text.data(), text.size());
    }

    // Reserves space for a length that is only known after the payload is
    // written, then patches it. This is what lets a reader skip a field it
    // does not understand.
    [[nodiscard]] std::size_t beginPatchableLength()
    {
        const std::size_t position = buffer_.size();
        writeRaw(std::uint32_t{0});
        return position;
    }

    void endPatchableLength(std::size_t position)
    {
        const auto length = static_cast<std::uint32_t>(buffer_.size() - position - sizeof(std::uint32_t));
        std::memcpy(buffer_.data() + position, &length, sizeof(length));
    }

    [[nodiscard]] const std::vector<std::uint8_t>& bytes() const noexcept { return buffer_; }
    [[nodiscard]] std::vector<std::uint8_t>        take() noexcept { return std::move(buffer_); }
    [[nodiscard]] std::size_t                      size() const noexcept { return buffer_.size(); }

private:
    std::vector<std::uint8_t> buffer_;
};

class ByteReader {
public:
    explicit ByteReader(std::span<const std::uint8_t> data) : data_(data) {}

    template <typename T>
    [[nodiscard]] bool readRaw(T& outValue)
    {
        static_assert(std::is_trivially_copyable_v<T>, "readRaw needs a trivially copyable type");
        if (remaining() < sizeof(T)) {
            failed_ = true;
            return false;
        }
        std::memcpy(&outValue, data_.data() + cursor_, sizeof(T));
        cursor_ += sizeof(T);
        return true;
    }

    [[nodiscard]] bool readBytes(void* out, std::size_t size)
    {
        if (remaining() < size) {
            failed_ = true;
            return false;
        }
        if (size > 0) {
            std::memcpy(out, data_.data() + cursor_, size);
        }
        cursor_ += size;
        return true;
    }

    [[nodiscard]] bool readString(std::string& out)
    {
        std::uint32_t length = 0;
        if (!readRaw(length)) {
            return false;
        }
        if (remaining() < length) {
            failed_ = true;
            return false;
        }
        out.assign(reinterpret_cast<const char*>(data_.data() + cursor_), length);
        cursor_ += length;
        return true;
    }

    [[nodiscard]] bool skip(std::size_t size)
    {
        if (remaining() < size) {
            failed_ = true;
            return false;
        }
        cursor_ += size;
        return true;
    }

    [[nodiscard]] std::size_t position() const noexcept  { return cursor_; }
    [[nodiscard]] std::size_t remaining() const noexcept { return data_.size() - cursor_; }
    [[nodiscard]] bool        failed() const noexcept    { return failed_; }
    [[nodiscard]] bool        exhausted() const noexcept { return cursor_ >= data_.size(); }

private:
    std::span<const std::uint8_t> data_;
    std::size_t                   cursor_ = 0;
    bool                          failed_ = false;
};

} // namespace harpia::serial
