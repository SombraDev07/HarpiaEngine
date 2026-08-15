// Buffer allocation, staged upload and mesh residency. The point is proving
// the bytes actually reached device-local memory, which only a readback shows.

#include <doctest/doctest.h>

#include "Core/Assets/MeshAsset.h"
#include "RHI/GpuMesh.h"
#include "RHI/Vulkan/VulkanBuffer.h"
#include "RHI/Vulkan/VulkanDevice.h"
#include "RHI/Vulkan/VulkanRenderer.h"

#include <cstring>
#include <numeric>
#include <vector>

using namespace harpia;

namespace {

struct GpuFixture {
    rhi::VulkanDevice   device;
    rhi::VulkanRenderer renderer;
    rhi::GpuUploader    uploader;
    bool                ready = false;

    GpuFixture()
    {
        rhi::DeviceDesc desc;
        desc.applicationName  = "HarpiaMeshTest";
        desc.enableValidation = true;
        desc.window           = nullptr;

        if (!device.create(desc)) {
            return;
        }
        // The renderer is here only for its bindless heap.
        if (!renderer.createOffscreen(device, 16, 16)) {
            return;
        }
        if (!uploader.create(device)) {
            return;
        }
        ready = true;
    }

    ~GpuFixture()
    {
        if (ready) {
            uploader.destroy();
            renderer.destroy();
            device.destroy();
        }
    }
};

// A quad: four vertices, two triangles, split into two sub-meshes so the
// vertexOffset path is exercised.
MeshAsset makeQuad()
{
    MeshAsset mesh;
    mesh.vertices = {
        MeshVertex{Vec3{-1, -1, 0}, Vec3{0, 0, 1}, Vec4{1, 0, 0, 1}, Vec2{0, 0}},
        MeshVertex{Vec3{ 1, -1, 0}, Vec3{0, 0, 1}, Vec4{1, 0, 0, 1}, Vec2{1, 0}},
        MeshVertex{Vec3{ 1,  1, 0}, Vec3{0, 0, 1}, Vec4{1, 0, 0, 1}, Vec2{1, 1}},
        MeshVertex{Vec3{-1,  1, 0}, Vec3{0, 0, 1}, Vec4{1, 0, 0, 1}, Vec2{0, 1}},
    };
    mesh.indices = {0, 1, 2, 0, 2, 3};

    SubMesh sub;
    sub.firstIndex   = 0;
    sub.indexCount   = 6;
    sub.vertexOffset = 0;
    sub.material     = 0;
    mesh.subMeshes.push_back(sub);

    mesh.materials.emplace_back();
    mesh.recomputeBounds();
    return mesh;
}

} // namespace

TEST_SUITE_BEGIN("gpu");

TEST_CASE("a host-visible buffer is written straight through its mapping")
{
    GpuFixture fixture;
    if (!fixture.ready) {
        MESSAGE("no Vulkan device available — skipping");
        return;
    }

    rhi::BufferDesc desc;
    desc.size      = 256;
    desc.usage     = VK_BUFFER_USAGE_UNIFORM_BUFFER_BIT;
    desc.memory    = rhi::BufferMemory::HostVisible;
    desc.debugName = "Test_HostVisible";

    rhi::VulkanBuffer buffer;
    REQUIRE(buffer.create(fixture.device, desc));
    CHECK(buffer.valid());
    CHECK(buffer.size() == 256);
    REQUIRE(buffer.mapped() != nullptr);

    std::vector<std::uint32_t> source(64);
    std::iota(source.begin(), source.end(), 1000u);
    REQUIRE(fixture.uploader.upload(buffer, source.data(), source.size() * sizeof(std::uint32_t)));

    std::vector<std::uint32_t> readBack(64);
    REQUIRE(fixture.uploader.download(buffer, readBack.data(),
                                      readBack.size() * sizeof(std::uint32_t)));
    CHECK(readBack == source);
}

TEST_CASE("a staged upload reaches device-local memory")
{
    GpuFixture fixture;
    if (!fixture.ready) {
        MESSAGE("no Vulkan device available — skipping");
        return;
    }

    constexpr std::size_t kCount = 4096;
    std::vector<std::uint32_t> source(kCount);
    std::iota(source.begin(), source.end(), 7u);

    rhi::BufferDesc desc;
    desc.size      = source.size() * sizeof(std::uint32_t);
    desc.usage     = VK_BUFFER_USAGE_STORAGE_BUFFER_BIT;
    desc.memory    = rhi::BufferMemory::DeviceLocal;
    desc.debugName = "Test_DeviceLocal";

    rhi::VulkanBuffer buffer;
    REQUIRE(buffer.create(fixture.device, desc));
    CHECK(buffer.mapped() == nullptr); // device-local is not host-mapped

    REQUIRE(fixture.uploader.upload(buffer, source.data(), desc.size));

    std::vector<std::uint32_t> readBack(kCount);
    REQUIRE(fixture.uploader.download(buffer, readBack.data(), desc.size));
    CHECK(readBack == source);
}

TEST_CASE("uploading at an offset leaves the rest of the buffer alone")
{
    GpuFixture fixture;
    if (!fixture.ready) {
        MESSAGE("no Vulkan device available — skipping");
        return;
    }

    rhi::BufferDesc desc;
    desc.size      = 64 * sizeof(std::uint32_t);
    desc.usage     = VK_BUFFER_USAGE_STORAGE_BUFFER_BIT;
    desc.memory    = rhi::BufferMemory::DeviceLocal;
    desc.debugName = "Test_Offset";

    rhi::VulkanBuffer buffer;
    REQUIRE(buffer.create(fixture.device, desc));

    std::vector<std::uint32_t> zeros(64, 0);
    REQUIRE(fixture.uploader.upload(buffer, zeros.data(), desc.size));

    const std::vector<std::uint32_t> patch{111, 222, 333};
    REQUIRE(fixture.uploader.upload(buffer, patch.data(),
                                    patch.size() * sizeof(std::uint32_t),
                                    16 * sizeof(std::uint32_t)));

    std::vector<std::uint32_t> readBack(64);
    REQUIRE(fixture.uploader.download(buffer, readBack.data(), desc.size));

    CHECK(readBack[15] == 0);
    CHECK(readBack[16] == 111);
    CHECK(readBack[18] == 333);
    CHECK(readBack[19] == 0);
}

TEST_CASE("an upload that would overrun the buffer is refused")
{
    GpuFixture fixture;
    if (!fixture.ready) {
        MESSAGE("no Vulkan device available — skipping");
        return;
    }

    rhi::BufferDesc desc;
    desc.size      = 64;
    desc.usage     = VK_BUFFER_USAGE_STORAGE_BUFFER_BIT;
    desc.memory    = rhi::BufferMemory::DeviceLocal;
    desc.debugName = "Test_Overrun";

    rhi::VulkanBuffer buffer;
    REQUIRE(buffer.create(fixture.device, desc));

    std::vector<std::uint8_t> tooMuch(128, 0xAB);
    CHECK_FALSE(fixture.uploader.upload(buffer, tooMuch.data(), tooMuch.size()));
    CHECK_FALSE(fixture.uploader.upload(buffer, tooMuch.data(), 32, 48)); // 48+32 > 64
}

TEST_CASE("a mesh becomes resident and keeps its data byte for byte")
{
    rhi::VulkanDevice::resetValidationErrorCount();

    GpuFixture fixture;
    if (!fixture.ready) {
        MESSAGE("no Vulkan device available — skipping");
        return;
    }

    const MeshAsset asset = makeQuad();

    rhi::GpuMesh mesh;
    REQUIRE(mesh.create(fixture.device, fixture.uploader,
                        fixture.renderer.bindless(), asset, "TestQuad"));

    CHECK(mesh.valid());
    CHECK(mesh.vertexCount() == 4);
    CHECK(mesh.indexCount() == 6);
    REQUIRE(mesh.subMeshes().size() == 1);
    CHECK(mesh.subMeshes()[0].indexCount == 6);

    // The vertex buffer went into the bindless heap, so a shader reaches it by
    // index rather than through a bound vertex buffer.
    CHECK(mesh.vertexBufferIndex() != rhi::VulkanBindless::kInvalidIndex);

    const rhi::MeshDrawConstants constants = mesh.drawConstants(0);
    CHECK(constants.vertexBufferIndex == mesh.vertexBufferIndex());
    CHECK(constants.materialIndex == 0);

    CHECK(mesh.bounds().min.x == doctest::Approx(-1.0f));
    CHECK(mesh.bounds().max.y == doctest::Approx(1.0f));

    CHECK(rhi::VulkanDevice::validationErrorCount() == 0);
}

TEST_CASE("a mesh releases its bindless slot when destroyed")
{
    GpuFixture fixture;
    if (!fixture.ready) {
        MESSAGE("no Vulkan device available — skipping");
        return;
    }

    const MeshAsset asset = makeQuad();
    rhi::VulkanBindless& bindless = fixture.renderer.bindless();

    const std::uint32_t before = bindless.usage().storageBuffers;

    std::uint32_t slot = rhi::VulkanBindless::kInvalidIndex;
    {
        rhi::GpuMesh mesh;
        REQUIRE(mesh.create(fixture.device, fixture.uploader, bindless, asset, "Scoped"));
        slot = mesh.vertexBufferIndex();
        CHECK(bindless.usage().storageBuffers == before + 1);
    }

    CHECK(bindless.usage().storageBuffers == before);

    // The freed slot is handed back out rather than leaked.
    rhi::GpuMesh second;
    REQUIRE(second.create(fixture.device, fixture.uploader, bindless, asset, "Reused"));
    CHECK(second.vertexBufferIndex() == slot);
}

TEST_CASE("an empty mesh is refused rather than uploaded")
{
    GpuFixture fixture;
    if (!fixture.ready) {
        MESSAGE("no Vulkan device available — skipping");
        return;
    }

    const MeshAsset empty;
    rhi::GpuMesh mesh;
    CHECK_FALSE(mesh.create(fixture.device, fixture.uploader,
                            fixture.renderer.bindless(), empty, "Empty"));
    CHECK_FALSE(mesh.valid());
}

TEST_SUITE_END();
