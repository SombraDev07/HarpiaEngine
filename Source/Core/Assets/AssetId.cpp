#include "Core/Assets/AssetId.h"

#include <array>
#include <mutex>
#include <random>

namespace harpia {
namespace {

std::mt19937_64& generator()
{
    // Seeded once per process from the system entropy source. Guarded because
    // asset import runs on job threads.
    static std::mt19937_64 engine{std::random_device{}()};
    return engine;
}

std::mutex& generatorMutex()
{
    static std::mutex mutex;
    return mutex;
}

[[nodiscard]] constexpr int hexValue(char c) noexcept
{
    if (c >= '0' && c <= '9') { return c - '0'; }
    if (c >= 'a' && c <= 'f') { return c - 'a' + 10; }
    if (c >= 'A' && c <= 'F') { return c - 'A' + 10; }
    return -1;
}

} // namespace

AssetId AssetId::generate()
{
    std::lock_guard<std::mutex> lock(generatorMutex());

    AssetId id;
    do {
        id.high = generator()();
        id.low  = generator()();
    } while (!id.valid()); // an all-zero draw would mean "invalid"

    return id;
}

std::string AssetId::toString() const
{
    static constexpr std::array<char, 16> kDigits{
        '0','1','2','3','4','5','6','7','8','9','a','b','c','d','e','f'};

    std::string text(32, '0');

    // 16 hex digits per 64-bit half means shifts of 60, 56, ... 0. Starting at
    // 56 both drops the top nibble and, on the last iteration, shifts by a
    // negative amount — undefined behaviour that silently corrupts every GUID.
    const auto writeHalf = [&text](std::uint64_t value, std::size_t offset) {
        for (int i = 0; i < 16; ++i) {
            const int shift = 60 - i * 4;
            const auto nibble = static_cast<std::uint8_t>((value >> shift) & 0xFu);
            text[offset + static_cast<std::size_t>(i)] = kDigits[nibble];
        }
    };

    writeHalf(high, 0);
    writeHalf(low, 16);
    return text;
}

AssetId AssetId::parse(std::string_view text)
{
    if (text.size() != 32) {
        return AssetId{};
    }

    AssetId id;
    for (std::size_t i = 0; i < 16; ++i) {
        const int value = hexValue(text[i]);
        if (value < 0) {
            return AssetId{};
        }
        id.high = (id.high << 4) | static_cast<std::uint64_t>(value);
    }
    for (std::size_t i = 16; i < 32; ++i) {
        const int value = hexValue(text[i]);
        if (value < 0) {
            return AssetId{};
        }
        id.low = (id.low << 4) | static_cast<std::uint64_t>(value);
    }
    return id;
}

} // namespace harpia
