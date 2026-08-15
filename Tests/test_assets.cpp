// The claim this file has to prove: renaming or moving a source file does not
// break a single reference to it. Everything else in the asset system is
// convenience; that guarantee is the reason GUIDs exist at all.

#include <doctest/doctest.h>

#include "Core/Assets/AssetDatabase.h"
#include "Core/Assets/AssetManager.h"

#include <filesystem>
#include <fstream>
#include <set>
#include <string>

using namespace harpia;
namespace fs = std::filesystem;

namespace {

// A scratch project tree that cleans itself up.
struct TempProject {
    fs::path root;

    TempProject()
    {
        const AssetId unique = AssetId::generate();
        root = fs::temp_directory_path() / ("harpia_assets_" + unique.toString());
        fs::create_directories(root);
    }

    ~TempProject()
    {
        std::error_code error;
        fs::remove_all(root, error);
    }

    fs::path write(const std::string& relative, const std::string& contents) const
    {
        const fs::path path = root / relative;
        fs::create_directories(path.parent_path());
        std::ofstream file(path, std::ios::trunc);
        file << contents;
        return path;
    }

    // Moves a source file together with its sidecar, which is what a user does
    // through the editor or a well-behaved VCS.
    void moveWithSidecar(const std::string& from, const std::string& to) const
    {
        const fs::path source = root / from;
        const fs::path target = root / to;
        fs::create_directories(target.parent_path());

        fs::rename(source, target);
        const fs::path sidecar = AssetDatabase::sidecarPathFor(source);
        if (fs::exists(sidecar)) {
            fs::rename(sidecar, AssetDatabase::sidecarPathFor(target));
        }
    }
};

} // namespace

TEST_CASE("generated ids are valid, unique and survive a text round trip")
{
    std::set<std::string> seen;
    for (int i = 0; i < 1000; ++i) {
        const AssetId id = AssetId::generate();
        REQUIRE(id.valid());

        const std::string text = id.toString();
        CHECK(text.size() == 32);
        CHECK(seen.insert(text).second); // no collision in 1000 draws

        CHECK(AssetId::parse(text) == id);
    }
}

TEST_CASE("malformed ids parse to invalid rather than to garbage")
{
    CHECK_FALSE(AssetId::parse("").valid());
    CHECK_FALSE(AssetId::parse("tooshort").valid());
    CHECK_FALSE(AssetId::parse(std::string(33, 'a')).valid());
    CHECK_FALSE(AssetId::parse(std::string(32, 'z')).valid()); // not hex
    CHECK_FALSE(AssetId{}.valid());
}

TEST_CASE("a scan discovers assets and writes one sidecar each")
{
    TempProject project;
    project.write("textures/brick.png", "fake png");
    project.write("meshes/cube.gltf", "fake gltf");
    project.write("notes.txt", "hello");
    project.write("ignored.bin", "unknown extension");

    AssetDatabase database;
    REQUIRE(database.open(project.root));

    const AssetDatabase::ScanResult result = database.scan();

    CHECK(result.discovered == 3);   // png, gltf, txt — not the .bin
    CHECK(result.reused == 0);
    CHECK(result.failed == 0);
    CHECK(database.size() == 3);

    CHECK(fs::exists(project.root / "textures/brick.png.meta"));
    CHECK(fs::exists(project.root / "meshes/cube.gltf.meta"));
    CHECK_FALSE(fs::exists(project.root / "ignored.bin.meta"));
}

TEST_CASE("a second scan reuses sidecars instead of reissuing ids")
{
    TempProject project;
    project.write("textures/brick.png", "fake png");

    AssetDatabase database;
    REQUIRE(database.open(project.root));

    database.scan();
    const AssetId first = database.idOf(project.root / "textures/brick.png");
    REQUIRE(first.valid());

    const AssetDatabase::ScanResult second = database.scan();
    CHECK(second.discovered == 0);
    CHECK(second.reused == 1);

    CHECK(database.idOf(project.root / "textures/brick.png") == first);
}

TEST_CASE("renaming a file keeps its id and updates its path")
{
    TempProject project;
    project.write("textures/brick.png", "fake png");

    AssetDatabase database;
    REQUIRE(database.open(project.root));
    database.scan();

    const AssetId id = database.idOf(project.root / "textures/brick.png");
    REQUIRE(id.valid());

    project.moveWithSidecar("textures/brick.png", "textures/wall.png");

    const AssetDatabase::ScanResult result = database.scan();

    CHECK(result.moved == 1);
    CHECK(result.discovered == 0);
    CHECK(database.size() == 1);

    // Same identity, new location: every reference held elsewhere still resolves.
    REQUIRE(database.contains(id));
    CHECK(database.pathOf(id).filename() == "wall.png");
    CHECK(database.idOf(project.root / "textures/wall.png") == id);
    CHECK_FALSE(database.idOf(project.root / "textures/brick.png").valid());
}

TEST_CASE("moving a file to another directory keeps its id")
{
    TempProject project;
    project.write("staging/hero.gltf", "fake gltf");

    AssetDatabase database;
    REQUIRE(database.open(project.root));
    database.scan();

    const AssetId id = database.idOf(project.root / "staging/hero.gltf");
    REQUIRE(id.valid());

    project.moveWithSidecar("staging/hero.gltf", "characters/final/hero.gltf");

    const AssetDatabase::ScanResult result = database.scan();

    CHECK(result.moved == 1);
    REQUIRE(database.contains(id));
    CHECK(database.pathOf(id) == project.root / "characters/final/hero.gltf");
}

TEST_CASE("a deleted file drops out of the index")
{
    TempProject project;
    project.write("a.png", "one");
    project.write("b.png", "two");

    AssetDatabase database;
    REQUIRE(database.open(project.root));
    database.scan();
    REQUIRE(database.size() == 2);

    const AssetId removed = database.idOf(project.root / "a.png");
    fs::remove(project.root / "a.png");
    fs::remove(project.root / "a.png.meta");

    const AssetDatabase::ScanResult result = database.scan();

    CHECK(result.missing == 1);
    CHECK(database.size() == 1);
    CHECK_FALSE(database.contains(removed));
}

TEST_CASE("a source file that loses its sidecar is treated as new")
{
    TempProject project;
    project.write("orphan.png", "content");

    AssetDatabase database;
    REQUIRE(database.open(project.root));
    database.scan();

    const AssetId original = database.idOf(project.root / "orphan.png");
    REQUIRE(original.valid());

    // Deleting the sidecar is how identity is genuinely lost. Nothing can
    // recover it, which is exactly why sidecars belong in version control.
    fs::remove(AssetDatabase::sidecarPathFor(project.root / "orphan.png"));

    const AssetDatabase::ScanResult result = database.scan();

    CHECK(result.discovered == 1);
    CHECK(result.missing == 1);
    CHECK_FALSE(database.contains(original));
    CHECK(database.idOf(project.root / "orphan.png") != original);
}

TEST_CASE("the index round trips through disk")
{
    TempProject project;
    project.write("textures/a.png", "a");
    project.write("meshes/b.gltf", "b");
    project.write("docs/c.txt", "c");

    const fs::path indexFile = project.root / "assets.db";

    AssetId first;
    {
        AssetDatabase database;
        REQUIRE(database.open(project.root));
        database.scan();
        first = database.idOf(project.root / "textures/a.png");
        REQUIRE(database.saveIndex(indexFile));
    }

    AssetDatabase reloaded;
    REQUIRE(reloaded.open(project.root));
    REQUIRE(reloaded.loadIndex(indexFile));

    CHECK(reloaded.size() == 3);
    REQUIRE(reloaded.contains(first));
    CHECK(reloaded.pathOf(first) == project.root / "textures/a.png");

    const AssetRecord* record = reloaded.find(first);
    REQUIRE(record != nullptr);
    CHECK(record->type == AssetType::Texture);
    CHECK(record->sourceSize == 1);
}

TEST_CASE("a missing or corrupt index fails without losing identities")
{
    TempProject project;
    project.write("a.png", "a");

    AssetDatabase database;
    REQUIRE(database.open(project.root));
    database.scan();
    const AssetId id = database.idOf(project.root / "a.png");

    CHECK_FALSE(database.loadIndex(project.root / "does-not-exist.db"));

    // The index is a cache: a rescan rebuilds it from the sidecars, so the
    // identity is never at risk.
    AssetDatabase rebuilt;
    REQUIRE(rebuilt.open(project.root));
    rebuilt.scan();
    CHECK(rebuilt.idOf(project.root / "a.png") == id);
}

TEST_CASE("extension mapping covers the types the roadmap names")
{
    CHECK(assetTypeForExtension(".png") == AssetType::Texture);
    CHECK(assetTypeForExtension(".PNG") == AssetType::Texture);
    CHECK(assetTypeForExtension(".ktx2") == AssetType::Texture);
    CHECK(assetTypeForExtension(".gltf") == AssetType::Mesh);
    CHECK(assetTypeForExtension(".glb") == AssetType::Mesh);
    CHECK(assetTypeForExtension(".hlsl") == AssetType::Shader);
    CHECK(assetTypeForExtension(".wav") == AssetType::Audio);
    CHECK(assetTypeForExtension(".zzz") == AssetType::Unknown);
}

TEST_CASE("assets load by id, not by path")
{
    TempProject project;
    project.write("docs/readme.txt", "the payload");

    AssetDatabase database;
    REQUIRE(database.open(project.root));
    database.scan();

    AssetManager manager;
    manager.attach(&database);
    manager.registerBinaryLoader(AssetType::Text);

    const AssetId id = database.idOf(project.root / "docs/readme.txt");
    REQUIRE(id.valid());

    const std::shared_ptr<BinaryAsset> asset = manager.load<BinaryAsset>(id);
    REQUIRE(asset != nullptr);
    CHECK(asset->text() == "the payload");
    CHECK(asset->id() == id);
    CHECK(asset->type() == AssetType::Text);
    CHECK(manager.stats().loads == 1);
}

TEST_CASE("a reference held across a rename still loads")
{
    TempProject project;
    project.write("docs/readme.txt", "still here");

    AssetDatabase database;
    REQUIRE(database.open(project.root));
    database.scan();

    AssetManager manager;
    manager.attach(&database);
    manager.registerBinaryLoader(AssetType::Text);

    // Something in a scene holds only this id — no path anywhere.
    const AssetId id = database.idOf(project.root / "docs/readme.txt");
    REQUIRE(manager.load<BinaryAsset>(id) != nullptr);

    manager.clear();
    project.moveWithSidecar("docs/readme.txt", "documentation/guide.txt");
    database.scan();

    const std::shared_ptr<BinaryAsset> after = manager.load<BinaryAsset>(id);
    REQUIRE(after != nullptr);
    CHECK(after->text() == "still here");
}

TEST_CASE("the cache serves repeat loads and releases on demand")
{
    TempProject project;
    project.write("docs/a.txt", "aaa");

    AssetDatabase database;
    REQUIRE(database.open(project.root));
    database.scan();

    AssetManager manager;
    manager.attach(&database);
    manager.registerBinaryLoader(AssetType::Text);

    const AssetId id = database.idOf(project.root / "docs/a.txt");

    std::shared_ptr<Asset> held = manager.load(id);
    REQUIRE(held != nullptr);
    CHECK(manager.load(id) == held);
    CHECK(manager.stats().loads == 1);
    CHECK(manager.stats().cacheHits == 1);
    CHECK(manager.loadedCount() == 1);

    // Still referenced outside the cache, so it stays.
    CHECK(manager.unloadUnused() == 0);

    held.reset();
    CHECK(manager.unloadUnused() == 1);
    CHECK(manager.loadedCount() == 0);
}

TEST_CASE("loading answers safely when it cannot succeed")
{
    TempProject project;
    project.write("docs/a.txt", "aaa");

    AssetDatabase database;
    REQUIRE(database.open(project.root));
    database.scan();

    AssetManager manager;

    SUBCASE("no database attached")
    {
        CHECK(manager.load(AssetId::generate()) == nullptr);
        CHECK(manager.stats().failures == 1);
    }

    SUBCASE("invalid id")
    {
        manager.attach(&database);
        CHECK(manager.load(AssetId{}) == nullptr);
        CHECK(manager.stats().failures == 1);
    }

    SUBCASE("unknown id")
    {
        manager.attach(&database);
        manager.registerBinaryLoader(AssetType::Text);
        CHECK(manager.load(AssetId::generate()) == nullptr);
        CHECK(manager.stats().failures == 1);
    }

    SUBCASE("no loader registered for the type")
    {
        manager.attach(&database);
        const AssetId id = database.idOf(project.root / "docs/a.txt");
        CHECK(manager.load(id) == nullptr);
        CHECK(manager.stats().failures == 1);
    }

    SUBCASE("wrong concrete type")
    {
        manager.attach(&database);
        manager.registerBinaryLoader(AssetType::Text);
        const AssetId id = database.idOf(project.root / "docs/a.txt");

        struct OtherAsset final : Asset {};
        // The load itself succeeds; only the cast fails, so this is not
        // counted as a load failure.
        CHECK(manager.load<OtherAsset>(id) == nullptr);
        CHECK(manager.load<BinaryAsset>(id) != nullptr);
        CHECK(manager.stats().failures == 0);
    }
}
