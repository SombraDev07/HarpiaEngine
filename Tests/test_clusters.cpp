// F2 step 5, first half: the cluster grid the light assignment will fill.
//
// The grid is pure geometry with no free parameters, so it gets checked the way
// the BRDF table was — against properties that must hold rather than against a
// picture. A grid that is subtly wrong does not look wrong; it drops lights
// near silhouettes and nowhere else, which is the hardest class of bug to
// attribute after the fact.

#include <doctest/doctest.h>

#include "Core/Math/Math.h"
#include "RHI/RenderTypes.h"
#include "RHI/Vulkan/VulkanBuffer.h"
#include "RHI/Vulkan/VulkanDevice.h"
#include "RHI/Vulkan/VulkanPipeline.h"
#include "RHI/Vulkan/VulkanRenderer.h"

#include <cmath>
#include <string>
#include <vector>

using namespace harpia;

TEST_SUITE_BEGIN("gpu");

TEST_CASE("the cluster grid tiles the frustum the way shading will read it")
{
    rhi::VulkanDevice::resetValidationErrorCount();

    rhi::DeviceDesc desc;
    desc.enableValidation = true;
    desc.window           = nullptr;

    rhi::VulkanDevice device;
    if (!device.create(desc)) {
        MESSAGE("no Vulkan device available — skipping");
        return;
    }

    rhi::VulkanRenderer renderer;
    rhi::GpuUploader    uploader;
    REQUIRE(renderer.createOffscreen(device, 16, 16));
    REQUIRE(uploader.create(device));

    constexpr float kNear   = 0.1f;
    constexpr float kFar    = 100.0f;
    constexpr float kWidth  = 1280.0f;
    constexpr float kHeight = 720.0f;

    rhi::VulkanBuffer clusters;
    rhi::BufferDesc bufferDesc;
    bufferDesc.size      = sizeof(rhi::GpuClusterBounds) * rhi::kClusterCount;
    bufferDesc.usage     = VK_BUFFER_USAGE_STORAGE_BUFFER_BIT;
    bufferDesc.memory    = rhi::BufferMemory::DeviceLocal;
    bufferDesc.debugName = "Test_Clusters";
    REQUIRE(clusters.create(device, bufferDesc));

    rhi::VulkanBindless& bindless = renderer.bindless();
    const std::uint32_t clusterIndex =
        bindless.registerStorageBuffer(clusters.handle(), 0, bufferDesc.size);
    REQUIRE(clusterIndex != rhi::VulkanBindless::kInvalidIndex);

    rhi::ComputePipelineDesc pipelineDesc;
    pipelineDesc.spirvPath           = std::string(HARPIA_SHADER_DIR) + "/ClusterBounds.comp.spv";
    pipelineDesc.descriptorSetLayout = bindless.layout();
    pipelineDesc.pushConstantBytes   = sizeof(rhi::ClusterPushConstants);
    pipelineDesc.debugName           = "Pipeline_ClusterBounds";

    rhi::VulkanComputePipeline pipeline;
    REQUIRE(pipeline.create(device, pipelineDesc));

    rhi::ClusterPushConstants push;
    push.clusterBuffer = clusterIndex;
    push.nearPlane     = kNear;
    push.farPlane      = kFar;
    push.tanHalfFov    = std::tan(radians(45.0f) * 0.5f);
    push.aspect        = kWidth / kHeight;
    push.renderSize    = Vec2(kWidth, kHeight);

    // --- dispatch ------------------------------------------------------------
    VkCommandPool pool = VK_NULL_HANDLE;
    VkCommandPoolCreateInfo poolInfo{};
    poolInfo.sType            = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO;
    poolInfo.flags            = VK_COMMAND_POOL_CREATE_TRANSIENT_BIT;
    poolInfo.queueFamilyIndex = device.graphics().family;
    REQUIRE(vkCreateCommandPool(device.device(), &poolInfo, nullptr, &pool) == VK_SUCCESS);

    VkCommandBuffer cmd = VK_NULL_HANDLE;
    VkCommandBufferAllocateInfo cmdAlloc{};
    cmdAlloc.sType              = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO;
    cmdAlloc.commandPool        = pool;
    cmdAlloc.level              = VK_COMMAND_BUFFER_LEVEL_PRIMARY;
    cmdAlloc.commandBufferCount = 1;
    REQUIRE(vkAllocateCommandBuffers(device.device(), &cmdAlloc, &cmd) == VK_SUCCESS);

    VkCommandBufferBeginInfo begin{};
    begin.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO;
    begin.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
    REQUIRE(vkBeginCommandBuffer(cmd, &begin) == VK_SUCCESS);

    const VkDescriptorSet set = bindless.set();
    vkCmdBindDescriptorSets(cmd, VK_PIPELINE_BIND_POINT_COMPUTE, pipeline.layout(),
                            0, 1, &set, 0, nullptr);
    pipeline.bind(cmd);
    vkCmdPushConstants(cmd, pipeline.layout(), VK_SHADER_STAGE_ALL, 0, sizeof(push), &push);
    rhi::VulkanComputePipeline::dispatchCovering(cmd, rhi::kClustersX, rhi::kClustersY,
                                                 rhi::kClustersZ, 4, 4, 4);

    VkBufferMemoryBarrier2 barrier{};
    barrier.sType         = VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER_2;
    barrier.srcStageMask  = VK_PIPELINE_STAGE_2_COMPUTE_SHADER_BIT;
    barrier.srcAccessMask = VK_ACCESS_2_SHADER_STORAGE_WRITE_BIT;
    barrier.dstStageMask  = VK_PIPELINE_STAGE_2_ALL_COMMANDS_BIT;
    barrier.dstAccessMask = VK_ACCESS_2_MEMORY_READ_BIT;
    barrier.srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    barrier.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    barrier.buffer        = clusters.handle();
    barrier.size          = bufferDesc.size;

    VkDependencyInfo dependency{};
    dependency.sType                    = VK_STRUCTURE_TYPE_DEPENDENCY_INFO;
    dependency.bufferMemoryBarrierCount = 1;
    dependency.pBufferMemoryBarriers    = &barrier;
    vkCmdPipelineBarrier2(cmd, &dependency);

    REQUIRE(vkEndCommandBuffer(cmd) == VK_SUCCESS);

    VkCommandBufferSubmitInfo cmdSubmit{};
    cmdSubmit.sType         = VK_STRUCTURE_TYPE_COMMAND_BUFFER_SUBMIT_INFO;
    cmdSubmit.commandBuffer = cmd;

    VkSubmitInfo2 submit{};
    submit.sType                  = VK_STRUCTURE_TYPE_SUBMIT_INFO_2;
    submit.commandBufferInfoCount = 1;
    submit.pCommandBufferInfos    = &cmdSubmit;

    REQUIRE(vkQueueSubmit2(device.graphics().queue, 1, &submit, VK_NULL_HANDLE) == VK_SUCCESS);
    REQUIRE(vkQueueWaitIdle(device.graphics().queue) == VK_SUCCESS);
    vkDestroyCommandPool(device.device(), pool, nullptr);

    std::vector<rhi::GpuClusterBounds> bounds(rhi::kClusterCount);
    REQUIRE(uploader.download(clusters, bounds.data(), bufferDesc.size));

    const auto at = [&](std::uint32_t x, std::uint32_t y, std::uint32_t z)
        -> const rhi::GpuClusterBounds& {
        return bounds[x + rhi::kClustersX * (y + rhi::kClustersY * z)];
    };

    SUBCASE("every cluster is a well-formed box")
    {
        // A single inverted axis anywhere means a sphere test that silently
        // rejects everything for that cell — lights vanishing in one band of
        // the screen and nowhere else.
        for (const rhi::GpuClusterBounds& box : bounds) {
            REQUIRE(box.maxPoint.x >= box.minPoint.x);
            REQUIRE(box.maxPoint.y >= box.minPoint.y);
            REQUIRE(box.maxPoint.z >= box.minPoint.z);
        }
    }

    SUBCASE("the grid spans exactly the near and far planes")
    {
        const rhi::GpuClusterBounds& first = at(0, 0, 0);
        const rhi::GpuClusterBounds& last  = at(0, 0, rhi::kClustersZ - 1);

        CHECK(first.minPoint.z == doctest::Approx(kNear).epsilon(0.01));
        CHECK(last.maxPoint.z == doctest::Approx(kFar).epsilon(0.01));
    }

    SUBCASE("depth slices grow geometrically, not linearly")
    {
        // The property that makes a fixed cluster count work at every scale: an
        // equal ratio between consecutive slices, so the near field gets as
        // many cells as the far field. A linear grid would put almost every
        // cell where nothing is, and leave the near field — where lights
        // actually overlap — in one or two.
        const float ratio = std::pow(kFar / kNear, 1.0f / rhi::kClustersZ);

        for (std::uint32_t z = 0; z + 1 < rhi::kClustersZ; ++z) {
            const float thisDepth = at(0, 0, z).maxPoint.z;
            const float nextDepth = at(0, 0, z + 1).maxPoint.z;
            CAPTURE(z);
            CHECK(nextDepth / thisDepth == doctest::Approx(ratio).epsilon(0.02));
        }
    }

    SUBCASE("neighbouring clusters leave no gap between them")
    {
        // Sideways the boxes must *overlap*, and asserting equality here is
        // what the first version of this test got wrong. Each cluster bounds a
        // truncated pyramid that widens with depth, so its maxX is measured at
        // the far slice while its neighbour's minX is measured at the near one.
        // The AABB is deliberately conservative; overlap costs a light being
        // tested twice, and a gap would be a seam lights fall through. Only the
        // gap is a bug.
        for (std::uint32_t x = 0; x + 1 < rhi::kClustersX; ++x) {
            CAPTURE(x);
            CHECK(at(x, 4, 0).maxPoint.x >= at(x + 1, 4, 0).minPoint.x);
        }
        for (std::uint32_t z = 0; z + 1 < rhi::kClustersZ; ++z) {
            CAPTURE(z);
            CHECK(at(0, 0, z).maxPoint.z
                  == doctest::Approx(at(0, 0, z + 1).minPoint.z).epsilon(0.01));
        }
    }

    SUBCASE("the grid is symmetric about the view axis")
    {
        // The projection is centred, so the leftmost and rightmost columns have
        // to mirror. An asymmetry here means the screen-to-view mapping picked
        // up an off-by-one in the tile size.
        const rhi::GpuClusterBounds& left  = at(0, 4, 5);
        const rhi::GpuClusterBounds& right = at(rhi::kClustersX - 1, 4, 5);
        CHECK(left.minPoint.x == doctest::Approx(-right.maxPoint.x).epsilon(0.01));
    }

    CHECK(rhi::VulkanDevice::validationErrorCount() == 0);

    pipeline.destroy();
    clusters.destroy();
    uploader.destroy();
    renderer.destroy();
    device.destroy();
}

TEST_CASE("light assignment puts a light in exactly the clusters it reaches")
{
    rhi::VulkanDevice::resetValidationErrorCount();

    rhi::DeviceDesc desc;
    desc.enableValidation = true;
    desc.window           = nullptr;

    rhi::VulkanDevice device;
    if (!device.create(desc)) {
        MESSAGE("no Vulkan device available — skipping");
        return;
    }

    rhi::VulkanRenderer renderer;
    rhi::GpuUploader    uploader;
    REQUIRE(renderer.createOffscreen(device, 16, 16));
    REQUIRE(uploader.create(device));

    constexpr float kNear = 0.1f;
    constexpr float kFar  = 100.0f;

    rhi::VulkanBindless& bindless = renderer.bindless();

    const auto makeBuffer = [&](rhi::VulkanBuffer& buffer, VkDeviceSize size,
                                const char* name) {
        rhi::BufferDesc d;
        d.size = size;
        d.usage = VK_BUFFER_USAGE_STORAGE_BUFFER_BIT;
        d.memory = rhi::BufferMemory::DeviceLocal;
        d.debugName = name;
        return buffer.create(device, d);
    };

    // Two lights: one straight ahead well inside the frustum, one far behind
    // the camera. The second is the assertion that matters — a culling bug that
    // accepts everything still lights the scene correctly and only shows up as
    // a frame time nobody can explain.
    std::vector<rhi::GpuPunctualLight> lights(2);
    lights[0].positionRange = Vec4(0.0f, 0.0f, 5.0f, 3.0f);
    lights[1].positionRange = Vec4(0.0f, 0.0f, -50.0f, 3.0f);

    rhi::VulkanBuffer clusterBuffer, lightBuffer, indexBuffer;
    REQUIRE(makeBuffer(clusterBuffer, sizeof(rhi::GpuClusterBounds) * rhi::kClusterCount,
                       "Test_ClusterBounds"));
    REQUIRE(makeBuffer(lightBuffer, sizeof(rhi::GpuPunctualLight) * lights.size(),
                       "Test_Lights"));
    const VkDeviceSize indexBytes =
        sizeof(std::uint32_t) * rhi::kClusterCount * (rhi::kMaxLightsPerCluster + 1);
    REQUIRE(makeBuffer(indexBuffer, indexBytes, "Test_LightIndices"));
    REQUIRE(uploader.upload(lightBuffer, lights.data(),
                            sizeof(rhi::GpuPunctualLight) * lights.size()));

    const std::uint32_t clusterIndex =
        bindless.registerStorageBuffer(clusterBuffer.handle(), 0, clusterBuffer.size());
    const std::uint32_t lightIndex =
        bindless.registerStorageBuffer(lightBuffer.handle(), 0, lightBuffer.size());
    const std::uint32_t listIndex =
        bindless.registerStorageBuffer(indexBuffer.handle(), 0, indexBytes);

    rhi::ComputePipelineDesc boundsDesc;
    boundsDesc.spirvPath = std::string(HARPIA_SHADER_DIR) + "/ClusterBounds.comp.spv";
    boundsDesc.descriptorSetLayout = bindless.layout();
    boundsDesc.pushConstantBytes   = sizeof(rhi::ClusterPushConstants);
    boundsDesc.debugName           = "Pipeline_Bounds";

    rhi::ComputePipelineDesc assignDesc;
    assignDesc.spirvPath = std::string(HARPIA_SHADER_DIR) + "/ClusterAssign.comp.spv";
    assignDesc.descriptorSetLayout = bindless.layout();
    assignDesc.pushConstantBytes   = sizeof(rhi::ClusterLightPushConstants);
    assignDesc.debugName           = "Pipeline_Assign";

    rhi::VulkanComputePipeline boundsPipeline, assignPipeline;
    REQUIRE(boundsPipeline.create(device, boundsDesc));
    REQUIRE(assignPipeline.create(device, assignDesc));

    rhi::ClusterPushConstants boundsPush;
    boundsPush.clusterBuffer = clusterIndex;
    boundsPush.nearPlane = kNear;
    boundsPush.farPlane  = kFar;
    boundsPush.tanHalfFov = std::tan(radians(45.0f) * 0.5f);
    boundsPush.aspect     = 16.0f / 9.0f;
    boundsPush.renderSize = Vec2(1280.0f, 720.0f);

    rhi::ClusterLightPushConstants assignPush;
    assignPush.clusterBuffer = clusterIndex;
    assignPush.lightBuffer   = lightIndex;
    assignPush.indexBuffer   = listIndex;
    assignPush.lightCount    = static_cast<std::uint32_t>(lights.size());
    assignPush.view          = Mat4(1.0f);   // camera at the origin looking down +Z

    VkCommandPool pool = VK_NULL_HANDLE;
    VkCommandPoolCreateInfo poolInfo{};
    poolInfo.sType = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO;
    poolInfo.flags = VK_COMMAND_POOL_CREATE_TRANSIENT_BIT;
    poolInfo.queueFamilyIndex = device.graphics().family;
    REQUIRE(vkCreateCommandPool(device.device(), &poolInfo, nullptr, &pool) == VK_SUCCESS);

    VkCommandBuffer cmd = VK_NULL_HANDLE;
    VkCommandBufferAllocateInfo cmdAlloc{};
    cmdAlloc.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO;
    cmdAlloc.commandPool = pool;
    cmdAlloc.level = VK_COMMAND_BUFFER_LEVEL_PRIMARY;
    cmdAlloc.commandBufferCount = 1;
    REQUIRE(vkAllocateCommandBuffers(device.device(), &cmdAlloc, &cmd) == VK_SUCCESS);

    VkCommandBufferBeginInfo begin{};
    begin.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO;
    begin.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
    REQUIRE(vkBeginCommandBuffer(cmd, &begin) == VK_SUCCESS);

    const VkDescriptorSet set = bindless.set();
    vkCmdBindDescriptorSets(cmd, VK_PIPELINE_BIND_POINT_COMPUTE, boundsPipeline.layout(),
                            0, 1, &set, 0, nullptr);
    boundsPipeline.bind(cmd);
    vkCmdPushConstants(cmd, boundsPipeline.layout(), VK_SHADER_STAGE_ALL, 0,
                       sizeof(boundsPush), &boundsPush);
    rhi::VulkanComputePipeline::dispatchCovering(cmd, rhi::kClustersX, rhi::kClustersY,
                                                 rhi::kClustersZ, 4, 4, 4);

    VkMemoryBarrier2 between{};
    between.sType = VK_STRUCTURE_TYPE_MEMORY_BARRIER_2;
    between.srcStageMask  = VK_PIPELINE_STAGE_2_COMPUTE_SHADER_BIT;
    between.srcAccessMask = VK_ACCESS_2_SHADER_STORAGE_WRITE_BIT;
    between.dstStageMask  = VK_PIPELINE_STAGE_2_COMPUTE_SHADER_BIT;
    between.dstAccessMask = VK_ACCESS_2_SHADER_STORAGE_READ_BIT;

    VkDependencyInfo dependency{};
    dependency.sType = VK_STRUCTURE_TYPE_DEPENDENCY_INFO;
    dependency.memoryBarrierCount = 1;
    dependency.pMemoryBarriers = &between;
    vkCmdPipelineBarrier2(cmd, &dependency);

    // Rebound, not reused. The two pipelines carry different push constant
    // sizes, which makes their layouts incompatible, and an incompatible layout
    // disturbs the descriptor set bound under the previous one. Vulkan says so
    // and the validation layer catches it — but only because it is checked.
    vkCmdBindDescriptorSets(cmd, VK_PIPELINE_BIND_POINT_COMPUTE, assignPipeline.layout(),
                            0, 1, &set, 0, nullptr);
    assignPipeline.bind(cmd);
    vkCmdPushConstants(cmd, assignPipeline.layout(), VK_SHADER_STAGE_ALL, 0,
                       sizeof(assignPush), &assignPush);
    rhi::VulkanComputePipeline::dispatchCovering(cmd, rhi::kClustersX, rhi::kClustersY,
                                                 rhi::kClustersZ, 4, 4, 4);

    between.dstStageMask  = VK_PIPELINE_STAGE_2_ALL_COMMANDS_BIT;
    between.dstAccessMask = VK_ACCESS_2_MEMORY_READ_BIT;
    vkCmdPipelineBarrier2(cmd, &dependency);

    REQUIRE(vkEndCommandBuffer(cmd) == VK_SUCCESS);

    VkCommandBufferSubmitInfo cmdSubmit{};
    cmdSubmit.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_SUBMIT_INFO;
    cmdSubmit.commandBuffer = cmd;
    VkSubmitInfo2 submit{};
    submit.sType = VK_STRUCTURE_TYPE_SUBMIT_INFO_2;
    submit.commandBufferInfoCount = 1;
    submit.pCommandBufferInfos = &cmdSubmit;
    REQUIRE(vkQueueSubmit2(device.graphics().queue, 1, &submit, VK_NULL_HANDLE) == VK_SUCCESS);
    REQUIRE(vkQueueWaitIdle(device.graphics().queue) == VK_SUCCESS);
    vkDestroyCommandPool(device.device(), pool, nullptr);

    std::vector<std::uint32_t> indices(rhi::kClusterCount * (rhi::kMaxLightsPerCluster + 1));
    REQUIRE(uploader.download(indexBuffer, indices.data(), indexBytes));

    const auto countAt = [&](std::uint32_t cluster) {
        return indices[cluster * (rhi::kMaxLightsPerCluster + 1)];
    };

    std::uint32_t withLight = 0;
    std::uint32_t behindCameraHits = 0;
    for (std::uint32_t c = 0; c < rhi::kClusterCount; ++c) {
        const std::uint32_t count = countAt(c);
        REQUIRE(count <= rhi::kMaxLightsPerCluster);
        if (count > 0) {
            ++withLight;
        }
        for (std::uint32_t i = 0; i < count; ++i) {
            if (indices[c * (rhi::kMaxLightsPerCluster + 1) + 1 + i] == 1) {
                ++behindCameraHits;
            }
        }
    }

    SUBCASE("the light in front reaches some clusters but nowhere near all")
    {
        // Culling that accepts everything still lights the scene correctly and
        // shows up only as a frame time nobody can explain. Both bounds matter.
        CHECK(withLight > 0);
        CHECK(withLight < rhi::kClusterCount / 2);
    }

    SUBCASE("a light behind the camera reaches nothing")
    {
        CHECK(behindCameraHits == 0);
    }

    SUBCASE("no cluster ever overflows its fixed slice")
    {
        // The reason there is no atomic pool: overflow here would be a write
        // past the buffer, and this bound is what makes that impossible.
        for (std::uint32_t c = 0; c < rhi::kClusterCount; ++c) {
            REQUIRE(countAt(c) <= rhi::kMaxLightsPerCluster);
        }
    }

    CHECK(rhi::VulkanDevice::validationErrorCount() == 0);

    boundsPipeline.destroy();
    assignPipeline.destroy();
    clusterBuffer.destroy();
    lightBuffer.destroy();
    indexBuffer.destroy();
    uploader.destroy();
    renderer.destroy();
    device.destroy();
}

TEST_SUITE_END();
