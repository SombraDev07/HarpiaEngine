// Harpia Engine — asset loading
//
// Everything is addressed by GUID. Nothing above this layer ever holds a path,
// which is what makes moving a file a non-event.
//
// Roadmap 1.7 stage 1: load straight from disk. The cache is already behind a
// mutex and loaders already take a path rather than reading it themselves, so
// stage 4 (async through the job system, plus hot reload) drops in without
// redesigning anything here.
#pragma once

#include "Core/Assets/AssetDatabase.h"
#include "Core/Assets/AssetId.h"

#include <cstdint>
#include <filesystem>
#include <functional>
#include <memory>
#include <mutex>
#include <string>
#include <unordered_map>
#include <vector>

namespace harpia {

// Base for anything the manager can hand out. Concrete assets derive from it
// and are recovered with load<T>.
class Asset {
public:
    virtual ~Asset() = default;

    [[nodiscard]] AssetId id() const noexcept { return id_; }
    [[nodiscard]] AssetType type() const noexcept { return type_; }

private:
    friend class AssetManager;
    AssetId   id_{};
    AssetType type_ = AssetType::Unknown;
};

// The simplest concrete asset: the file's bytes. Enough to prove the path from
// GUID to content end to end, and genuinely useful for shaders and configs.
class BinaryAsset final : public Asset {
public:
    explicit BinaryAsset(std::vector<std::uint8_t> bytes) : bytes_(std::move(bytes)) {}

    [[nodiscard]] const std::vector<std::uint8_t>& bytes() const noexcept { return bytes_; }
    [[nodiscard]] std::size_t size() const noexcept { return bytes_.size(); }
    [[nodiscard]] std::string text() const
    {
        return std::string(reinterpret_cast<const char*>(bytes_.data()), bytes_.size());
    }

private:
    std::vector<std::uint8_t> bytes_;
};

using AssetLoader = std::function<std::shared_ptr<Asset>(const std::filesystem::path&)>;

class AssetManager {
public:
    // The database is borrowed, not owned; the editor and the runtime share one.
    void attach(AssetDatabase* database) noexcept;

    // Later registrations replace earlier ones for the same type.
    void registerLoader(AssetType type, AssetLoader loader);

    // Registers a loader that reads the whole file into a BinaryAsset. Handy
    // default for types with no decoder yet.
    void registerBinaryLoader(AssetType type);

    [[nodiscard]] std::shared_ptr<Asset> load(AssetId id);

    template <typename T>
    [[nodiscard]] std::shared_ptr<T> load(AssetId id)
    {
        return std::dynamic_pointer_cast<T>(load(id));
    }

    // Drops cached assets nobody else is holding. Callers keep what they need
    // alive by holding the shared_ptr, so this can run whenever.
    std::size_t unloadUnused();

    void clear();

    [[nodiscard]] bool        isLoaded(AssetId id) const;
    [[nodiscard]] std::size_t loadedCount() const;

    struct Stats {
        std::uint64_t loads     = 0;  // went to disk
        std::uint64_t cacheHits = 0;
        std::uint64_t failures  = 0;
    };
    [[nodiscard]] Stats stats() const noexcept;

private:
    mutable std::mutex                                  mutex_;
    AssetDatabase*                                      database_ = nullptr;
    std::unordered_map<AssetType, AssetLoader>          loaders_;
    std::unordered_map<AssetId, std::shared_ptr<Asset>> cache_;

    std::uint64_t loads_     = 0;
    std::uint64_t cacheHits_ = 0;
    std::uint64_t failures_  = 0;
};

// Reads a whole file. Returns an empty vector when it cannot be opened.
[[nodiscard]] std::vector<std::uint8_t> readFileBytes(const std::filesystem::path& path);

} // namespace harpia
