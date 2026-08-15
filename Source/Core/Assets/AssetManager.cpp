#include "Core/Assets/AssetManager.h"

#include <cstdio>
#include <fstream>

namespace harpia {

std::vector<std::uint8_t> readFileBytes(const std::filesystem::path& path)
{
    std::ifstream file(path, std::ios::binary | std::ios::ate);
    if (!file) {
        return {};
    }

    const auto size = static_cast<std::size_t>(file.tellg());
    std::vector<std::uint8_t> bytes(size);
    file.seekg(0);
    if (size > 0) {
        file.read(reinterpret_cast<char*>(bytes.data()), static_cast<std::streamsize>(size));
    }
    return bytes;
}

void AssetManager::attach(AssetDatabase* database) noexcept
{
    std::lock_guard<std::mutex> lock(mutex_);
    database_ = database;
}

void AssetManager::registerLoader(AssetType type, AssetLoader loader)
{
    std::lock_guard<std::mutex> lock(mutex_);
    loaders_[type] = std::move(loader);
}

void AssetManager::registerBinaryLoader(AssetType type)
{
    registerLoader(type, [](const std::filesystem::path& path) -> std::shared_ptr<Asset> {
        std::vector<std::uint8_t> bytes = readFileBytes(path);
        if (bytes.empty()) {
            return nullptr;
        }
        return std::make_shared<BinaryAsset>(std::move(bytes));
    });
}

std::shared_ptr<Asset> AssetManager::load(AssetId id)
{
    if (!id.valid()) {
        std::lock_guard<std::mutex> lock(mutex_);
        ++failures_;
        return nullptr;
    }

    AssetLoader           loader;
    std::filesystem::path path;
    AssetType             type = AssetType::Unknown;

    {
        std::lock_guard<std::mutex> lock(mutex_);

        const auto cached = cache_.find(id);
        if (cached != cache_.end()) {
            ++cacheHits_;
            return cached->second;
        }

        if (database_ == nullptr) {
            ++failures_;
            return nullptr;
        }

        const AssetRecord* record = database_->find(id);
        if (record == nullptr) {
            ++failures_;
            return nullptr;
        }

        const auto found = loaders_.find(record->type);
        if (found == loaders_.end()) {
            ++failures_;
            return nullptr;
        }

        loader = found->second;
        path   = database_->pathOf(id);
        type   = record->type;
    }

    // The loader runs outside the lock: decoding a texture must not block every
    // other load, and this is the shape the async path will need anyway.
    std::shared_ptr<Asset> asset = loader(path);

    std::lock_guard<std::mutex> lock(mutex_);
    if (asset == nullptr) {
        ++failures_;
        return nullptr;
    }

    // Another thread may have finished the same asset while we were decoding.
    // Keeping the first one avoids handing out two objects for one GUID.
    const auto raced = cache_.find(id);
    if (raced != cache_.end()) {
        ++cacheHits_;
        return raced->second;
    }

    asset->id_   = id;
    asset->type_ = type;
    cache_[id]   = asset;
    ++loads_;
    return asset;
}

std::size_t AssetManager::unloadUnused()
{
    std::lock_guard<std::mutex> lock(mutex_);

    std::size_t removed = 0;
    for (auto it = cache_.begin(); it != cache_.end();) {
        if (it->second.use_count() == 1) { // only the cache holds it
            it = cache_.erase(it);
            ++removed;
        } else {
            ++it;
        }
    }
    return removed;
}

void AssetManager::clear()
{
    std::lock_guard<std::mutex> lock(mutex_);
    cache_.clear();
}

bool AssetManager::isLoaded(AssetId id) const
{
    std::lock_guard<std::mutex> lock(mutex_);
    return cache_.find(id) != cache_.end();
}

std::size_t AssetManager::loadedCount() const
{
    std::lock_guard<std::mutex> lock(mutex_);
    return cache_.size();
}

AssetManager::Stats AssetManager::stats() const noexcept
{
    std::lock_guard<std::mutex> lock(mutex_);
    return Stats{loads_, cacheHits_, failures_};
}

} // namespace harpia
