#include "Core/Assets/AssetDatabase.h"

#include "Core/Reflection/Reflect.h"
#include "Core/Serialization/Serializer.h"

#include <algorithm>
#include <cstdio>
#include <fstream>
#include <sstream>

// The index is serialised through our own reflection layer rather than a
// bespoke reader. Dogfooding it here is deliberate: if schema evolution does
// not hold up for the asset index, it will not hold up for scenes either.
HARPIA_REFLECT_BEGIN(harpia::AssetId, 1)
    HARPIA_FIELD(high)
    HARPIA_FIELD(low)
HARPIA_REFLECT_END(harpia::AssetId)

HARPIA_REFLECT_BEGIN(harpia::AssetRecord, 1)
    HARPIA_FIELD(id)
    HARPIA_FIELD(path)
    HARPIA_FIELD(type)
    HARPIA_FIELD(sourceSize)
HARPIA_REFLECT_END(harpia::AssetRecord)

namespace harpia {

// Wrapper so the whole index is one serialisable object.
struct AssetIndexFile {
    std::vector<AssetRecord> records;
};

} // namespace harpia

HARPIA_REFLECT_BEGIN(harpia::AssetIndexFile, 1)
    HARPIA_FIELD(records)
HARPIA_REFLECT_END(harpia::AssetIndexFile)

namespace harpia {
namespace {

namespace fs = std::filesystem;

constexpr std::string_view kSidecarExtension = ".meta";
constexpr std::string_view kSidecarHeader    = "harpia-meta";
constexpr int              kSidecarVersion   = 1;

[[nodiscard]] std::string toLower(std::string_view text)
{
    std::string lowered(text);
    std::transform(lowered.begin(), lowered.end(), lowered.begin(),
                   [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
    return lowered;
}

// Directories the scanner never descends into.
[[nodiscard]] bool isIgnoredDirectory(const std::string& name)
{
    return name == ".git" || name == "build" || name == "_output"
        || name == ".cache" || (!name.empty() && name.front() == '.');
}

} // namespace

const char* toString(AssetType type) noexcept
{
    switch (type) {
        case AssetType::Unknown:  return "unknown";
        case AssetType::Texture:  return "texture";
        case AssetType::Mesh:     return "mesh";
        case AssetType::Shader:   return "shader";
        case AssetType::Material: return "material";
        case AssetType::Scene:    return "scene";
        case AssetType::Audio:    return "audio";
        case AssetType::Text:     return "text";
    }
    return "unknown";
}

AssetType assetTypeForExtension(std::string_view extension) noexcept
{
    const std::string ext = toLower(extension);

    if (ext == ".png" || ext == ".jpg" || ext == ".jpeg" || ext == ".tga"
        || ext == ".dds" || ext == ".ktx2" || ext == ".hdr") {
        return AssetType::Texture;
    }
    if (ext == ".gltf" || ext == ".glb" || ext == ".obj" || ext == ".fbx") {
        return AssetType::Mesh;
    }
    if (ext == ".hlsl" || ext == ".spv") {
        return AssetType::Shader;
    }
    if (ext == ".hmat") {
        return AssetType::Material;
    }
    if (ext == ".hscene") {
        return AssetType::Scene;
    }
    if (ext == ".wav" || ext == ".ogg" || ext == ".flac") {
        return AssetType::Audio;
    }
    if (ext == ".txt" || ext == ".json" || ext == ".md") {
        return AssetType::Text;
    }
    return AssetType::Unknown;
}

fs::path AssetDatabase::sidecarPathFor(const fs::path& assetFile)
{
    fs::path sidecar = assetFile;
    sidecar += std::string(kSidecarExtension);
    return sidecar;
}

AssetId AssetDatabase::readSidecar(const fs::path& assetFile)
{
    std::ifstream file(sidecarPathFor(assetFile));
    if (!file) {
        return AssetId{};
    }

    std::string header;
    int         version = 0;
    file >> header >> version;
    if (header != kSidecarHeader || version < 1 || version > kSidecarVersion) {
        return AssetId{};
    }

    std::string key;
    while (file >> key) {
        if (key == "guid") {
            std::string value;
            file >> value;
            return AssetId::parse(value);
        }
        std::string ignored;
        std::getline(file, ignored);
    }
    return AssetId{};
}

bool AssetDatabase::writeSidecar(const fs::path& assetFile, AssetId id, AssetType type)
{
    std::ofstream file(sidecarPathFor(assetFile), std::ios::trunc);
    if (!file) {
        return false;
    }
    file << kSidecarHeader << ' ' << kSidecarVersion << '\n'
         << "guid " << id.toString() << '\n'
         << "type " << toString(type) << '\n';
    return file.good();
}

bool AssetDatabase::open(const fs::path& root)
{
    std::error_code error;
    if (!fs::is_directory(root, error)) {
        std::fprintf(stderr, "[assets] root is not a directory: %s\n", root.string().c_str());
        return false;
    }

    root_ = fs::weakly_canonical(root, error);
    if (error) {
        root_ = root;
    }
    records_.clear();
    byPath_.clear();
    open_ = true;
    return true;
}

void AssetDatabase::close()
{
    records_.clear();
    byPath_.clear();
    root_.clear();
    open_ = false;
}

std::string AssetDatabase::relativeOf(const fs::path& path) const
{
    std::error_code error;
    fs::path relative = fs::relative(path, root_, error);
    if (error || relative.empty()) {
        relative = path.filename();
    }
    // generic_string keeps '/' on every platform, so an index written on one
    // machine resolves on another.
    return relative.generic_string();
}

void AssetDatabase::index(const AssetRecord& record)
{
    records_[record.id] = record;
    byPath_[record.path] = record.id;
}

AssetDatabase::ScanResult AssetDatabase::scan()
{
    ScanResult result;
    if (!open_) {
        return result;
    }

    // Paths seen this pass; anything indexed but not seen has gone missing.
    std::unordered_map<AssetId, bool> seen;
    seen.reserve(records_.size());

    std::error_code error;
    fs::recursive_directory_iterator iterator(root_, fs::directory_options::skip_permission_denied,
                                              error);
    if (error) {
        return result;
    }

    for (auto entry = iterator; entry != fs::recursive_directory_iterator(); ++entry) {
        const fs::path& path = entry->path();

        if (entry->is_directory(error)) {
            if (isIgnoredDirectory(path.filename().string())) {
                entry.disable_recursion_pending();
            }
            continue;
        }
        if (!entry->is_regular_file(error)) {
            continue;
        }
        if (path.extension() == kSidecarExtension) {
            continue; // sidecars describe assets, they are not assets
        }

        const AssetType type = assetTypeForExtension(path.extension().string());
        if (type == AssetType::Unknown) {
            continue;
        }

        AssetId id = readSidecar(path);
        const bool hadSidecar = id.valid();

        if (!hadSidecar) {
            id = AssetId::generate();
            if (!writeSidecar(path, id, type)) {
                ++result.failed;
                continue;
            }
            ++result.discovered;
        } else {
            ++result.reused;
        }

        AssetRecord record;
        record.id   = id;
        record.path = relativeOf(path);
        record.type = type;
        record.sourceSize = static_cast<std::uint64_t>(fs::file_size(path, error));

        // The identity survived; only the location changed. This is the whole
        // reason the GUID lives in a sidecar instead of in the index.
        const auto existing = records_.find(id);
        if (existing != records_.end() && existing->second.path != record.path) {
            byPath_.erase(existing->second.path);
            ++result.moved;
        }

        index(record);
        seen[id] = true;
    }

    for (auto it = records_.begin(); it != records_.end();) {
        if (seen.find(it->first) == seen.end()) {
            byPath_.erase(it->second.path);
            it = records_.erase(it);
            ++result.missing;
        } else {
            ++it;
        }
    }

    return result;
}

const AssetRecord* AssetDatabase::find(AssetId id) const noexcept
{
    const auto it = records_.find(id);
    return it != records_.end() ? &it->second : nullptr;
}

bool AssetDatabase::contains(AssetId id) const noexcept
{
    return records_.find(id) != records_.end();
}

AssetId AssetDatabase::idOf(const fs::path& path) const
{
    const std::string relative = path.is_absolute() ? relativeOf(path) : path.generic_string();
    const auto it = byPath_.find(relative);
    return it != byPath_.end() ? it->second : AssetId{};
}

fs::path AssetDatabase::pathOf(AssetId id) const
{
    const AssetRecord* record = find(id);
    return record != nullptr ? root_ / record->path : fs::path{};
}

std::vector<AssetRecord> AssetDatabase::records() const
{
    std::vector<AssetRecord> all;
    all.reserve(records_.size());
    for (const auto& [id, record] : records_) {
        all.push_back(record);
    }
    // Stable order so a written index is byte-identical across runs, which
    // keeps it diffable and makes a spurious rewrite visible.
    std::sort(all.begin(), all.end(), [](const AssetRecord& a, const AssetRecord& b) {
        return a.id < b.id;
    });
    return all;
}

bool AssetDatabase::saveIndex(const fs::path& file) const
{
    AssetIndexFile index;
    index.records = records();

    const std::vector<std::uint8_t> bytes = serial::saveToBytes(index);

    std::ofstream out(file, std::ios::binary | std::ios::trunc);
    if (!out) {
        return false;
    }
    out.write(reinterpret_cast<const char*>(bytes.data()),
              static_cast<std::streamsize>(bytes.size()));
    return out.good();
}

bool AssetDatabase::loadIndex(const fs::path& file)
{
    std::ifstream in(file, std::ios::binary | std::ios::ate);
    if (!in) {
        return false;
    }

    const auto size = static_cast<std::size_t>(in.tellg());
    std::vector<std::uint8_t> bytes(size);
    in.seekg(0);
    in.read(reinterpret_cast<char*>(bytes.data()), static_cast<std::streamsize>(size));

    AssetIndexFile index;
    if (!serial::loadFromBytes(index, bytes)) {
        return false;
    }

    records_.clear();
    byPath_.clear();
    for (const AssetRecord& record : index.records) {
        this->index(record);
    }
    return true;
}

} // namespace harpia
