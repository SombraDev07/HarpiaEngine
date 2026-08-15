// Harpia Engine — asset database
//
// Two artefacts, on purpose:
//
//   <asset>.meta — one text sidecar per source file, holding its GUID. Text
//                  because it lives in version control next to the asset and
//                  has to diff and merge like source.
//   assets.db    — the binary index, rebuilt from the sidecars at will. Fast
//                  to load, never authoritative, safe to delete.
//
// The sidecar is what makes identity survive a move: the GUID travels with the
// file, and the index is just a cache of where that file currently is.
#pragma once

#include "Core/Assets/AssetId.h"

#include <cstdint>
#include <filesystem>
#include <string>
#include <string_view>
#include <unordered_map>
#include <vector>

namespace harpia {

enum class AssetType : std::uint16_t {
    Unknown = 0,
    Texture,
    Mesh,
    Shader,
    Material,
    Scene,
    Audio,
    Text,
};

[[nodiscard]] const char* toString(AssetType type) noexcept;
[[nodiscard]] AssetType   assetTypeForExtension(std::string_view extension) noexcept;

struct AssetRecord {
    AssetId       id;
    std::string   path;          // relative to the root, always '/' separated
    AssetType     type = AssetType::Unknown;
    std::uint64_t sourceSize = 0;
};

class AssetDatabase {
public:
    struct ScanResult {
        std::uint32_t discovered = 0; // no sidecar yet, one was written
        std::uint32_t reused     = 0; // sidecar already present
        std::uint32_t moved      = 0; // known GUID found at a new path
        std::uint32_t missing    = 0; // indexed record whose file is gone
        std::uint32_t failed     = 0; // sidecar unreadable or unwritable
    };

    [[nodiscard]] bool open(const std::filesystem::path& root);
    void close();

    // Walks the root, creating a sidecar for anything that lacks one and
    // refreshing paths for everything that has one.
    ScanResult scan();

    [[nodiscard]] const AssetRecord* find(AssetId id) const noexcept;
    [[nodiscard]] AssetId            idOf(const std::filesystem::path& path) const;
    [[nodiscard]] std::filesystem::path pathOf(AssetId id) const;
    [[nodiscard]] bool                  contains(AssetId id) const noexcept;

    // The index is a cache. Losing it costs a rescan, never an identity.
    [[nodiscard]] bool saveIndex(const std::filesystem::path& file) const;
    [[nodiscard]] bool loadIndex(const std::filesystem::path& file);

    [[nodiscard]] std::size_t size() const noexcept { return records_.size(); }
    [[nodiscard]] std::vector<AssetRecord> records() const;
    [[nodiscard]] const std::filesystem::path& root() const noexcept { return root_; }

    // Reads or writes the sidecar for one source file. Exposed so an importer
    // can claim a GUID before the next full scan.
    [[nodiscard]] static AssetId readSidecar(const std::filesystem::path& assetFile);
    [[nodiscard]] static bool    writeSidecar(const std::filesystem::path& assetFile,
                                              AssetId                      id,
                                              AssetType                    type);
    [[nodiscard]] static std::filesystem::path sidecarPathFor(
        const std::filesystem::path& assetFile);

private:
    [[nodiscard]] std::string relativeOf(const std::filesystem::path& path) const;
    void index(const AssetRecord& record);

    std::filesystem::path                       root_;
    std::unordered_map<AssetId, AssetRecord>    records_;
    std::unordered_map<std::string, AssetId>    byPath_;
    bool                                        open_ = false;
};

} // namespace harpia
