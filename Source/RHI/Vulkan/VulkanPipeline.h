// Harpia Engine — graphics pipeline
//
// Dynamic rendering means no render pass object and no framebuffer; a pipeline
// declares the attachment formats it targets and nothing else binds it to a
// particular render target.
#pragma once

#include "RHI/Vulkan/VulkanCommon.h"

#include <cstdint>
#include <string>
#include <vector>

namespace harpia::rhi {

class VulkanDevice;

// Loads SPIR-V produced by the build's shader step.
[[nodiscard]] std::vector<std::uint32_t> loadSpirv(const std::string& path);

struct GraphicsPipelineDesc {
    std::string vertexSpirvPath;
    std::string fragmentSpirvPath;

    std::vector<VkFormat> colorFormats;
    VkFormat              depthFormat = VK_FORMAT_UNDEFINED;

    VkPrimitiveTopology topology    = VK_PRIMITIVE_TOPOLOGY_TRIANGLE_LIST;
    VkCullModeFlags     cullMode    = VK_CULL_MODE_NONE;
    VkFrontFace         frontFace   = VK_FRONT_FACE_COUNTER_CLOCKWISE;
    bool                depthTest   = false;
    bool                depthWrite  = false;
    bool                blend       = false;

    // Bindless: the one global set, plus push constants for indices.
    VkDescriptorSetLayout descriptorSetLayout = VK_NULL_HANDLE;
    std::uint32_t         pushConstantBytes   = 0;

    const char* debugName = "Pipeline";
};

class VulkanPipeline {
public:
    VulkanPipeline() = default;
    ~VulkanPipeline();

    VulkanPipeline(const VulkanPipeline&)            = delete;
    VulkanPipeline& operator=(const VulkanPipeline&) = delete;

    [[nodiscard]] bool create(VulkanDevice& device, const GraphicsPipelineDesc& desc);
    void destroy();

    [[nodiscard]] VkPipeline       handle() const noexcept { return pipeline_; }
    [[nodiscard]] VkPipelineLayout layout() const noexcept { return layout_; }

    void bind(VkCommandBuffer cmd) const;

    // Viewport and scissor are dynamic, so one pipeline serves every render
    // target size and a resize never rebuilds pipelines.
    void setViewportAndScissor(VkCommandBuffer cmd, VkExtent2D extent) const;

private:
    VulkanDevice*    device_   = nullptr;
    VkPipeline       pipeline_ = VK_NULL_HANDLE;
    VkPipelineLayout layout_   = VK_NULL_HANDLE;
};

// Compute. Separate type rather than a mode on the graphics one: they share
// only the layout, and every other field of GraphicsPipelineDesc would be dead
// weight a reader has to know is dead.
struct ComputePipelineDesc {
    std::string spirvPath;

    VkDescriptorSetLayout descriptorSetLayout = VK_NULL_HANDLE;
    std::uint32_t         pushConstantBytes   = 0;

    const char* debugName = "ComputePipeline";
};

class VulkanComputePipeline {
public:
    VulkanComputePipeline() = default;
    ~VulkanComputePipeline();

    VulkanComputePipeline(const VulkanComputePipeline&)            = delete;
    VulkanComputePipeline& operator=(const VulkanComputePipeline&) = delete;

    [[nodiscard]] bool create(VulkanDevice& device, const ComputePipelineDesc& desc);
    void destroy();

    [[nodiscard]] VkPipeline       handle() const noexcept { return pipeline_; }
    [[nodiscard]] VkPipelineLayout layout() const noexcept { return layout_; }

    void bind(VkCommandBuffer cmd) const;

    // Rounds up, because a grid that is not a multiple of the group size still
    // has to be covered. The shader is responsible for ignoring the overhang —
    // dispatching short is a silent hole in the output.
    static void dispatchCovering(VkCommandBuffer cmd,
                                 std::uint32_t   width,
                                 std::uint32_t   height,
                                 std::uint32_t   depth,
                                 std::uint32_t   groupWidth,
                                 std::uint32_t   groupHeight = 1,
                                 std::uint32_t   groupDepth  = 1);

private:
    VulkanDevice*    device_   = nullptr;
    VkPipeline       pipeline_ = VK_NULL_HANDLE;
    VkPipelineLayout layout_   = VK_NULL_HANDLE;
};

} // namespace harpia::rhi
