#include "RHI/Vulkan/VulkanPipeline.h"

#include "RHI/Vulkan/VulkanDevice.h"

#include <cstdio>
#include <fstream>

namespace harpia::rhi {
namespace {

[[nodiscard]] VkShaderModule createModule(VkDevice device,
                                          const std::vector<std::uint32_t>& code,
                                          const char* name)
{
    if (code.empty()) {
        return VK_NULL_HANDLE;
    }

    VkShaderModuleCreateInfo info{};
    info.sType    = VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO;
    info.codeSize = code.size() * sizeof(std::uint32_t);
    info.pCode    = code.data();

    VkShaderModule module = VK_NULL_HANDLE;
    HARPIA_VK_CHECK(vkCreateShaderModule(device, &info, nullptr, &module));
    setDebugName(device, VK_OBJECT_TYPE_SHADER_MODULE, module, name);
    return module;
}

} // namespace

std::vector<std::uint32_t> loadSpirv(const std::string& path)
{
    std::ifstream file(path, std::ios::binary | std::ios::ate);
    if (!file) {
        std::fprintf(stderr, "[shader] cannot open %s\n", path.c_str());
        return {};
    }

    const auto size = static_cast<std::size_t>(file.tellg());
    if (size == 0 || size % sizeof(std::uint32_t) != 0) {
        std::fprintf(stderr, "[shader] %s is not a valid SPIR-V size (%zu bytes)\n",
                     path.c_str(), size);
        return {};
    }

    std::vector<std::uint32_t> code(size / sizeof(std::uint32_t));
    file.seekg(0);
    file.read(reinterpret_cast<char*>(code.data()), static_cast<std::streamsize>(size));

    // 0x07230203 is the SPIR-V magic; catching it here beats a driver crash.
    if (code[0] != 0x07230203u) {
        std::fprintf(stderr, "[shader] %s has a bad SPIR-V magic word\n", path.c_str());
        return {};
    }
    return code;
}

VulkanPipeline::~VulkanPipeline()
{
    destroy();
}

bool VulkanPipeline::create(VulkanDevice& device, const GraphicsPipelineDesc& desc)
{
    device_ = &device;
    const VkDevice handle = device.device();

    const std::vector<std::uint32_t> vertexCode   = loadSpirv(desc.vertexSpirvPath);
    const std::vector<std::uint32_t> fragmentCode = loadSpirv(desc.fragmentSpirvPath);
    if (vertexCode.empty() || fragmentCode.empty()) {
        return false;
    }

    VkShaderModule vertexModule   = createModule(handle, vertexCode, "Shader_Vertex");
    VkShaderModule fragmentModule = createModule(handle, fragmentCode, "Shader_Fragment");

    VkPipelineShaderStageCreateInfo stages[2]{};
    stages[0].sType  = VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO;
    stages[0].stage  = VK_SHADER_STAGE_VERTEX_BIT;
    stages[0].module = vertexModule;
    stages[0].pName  = "main";
    stages[1].sType  = VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO;
    stages[1].stage  = VK_SHADER_STAGE_FRAGMENT_BIT;
    stages[1].module = fragmentModule;
    stages[1].pName  = "main";

    // No vertex input state: geometry comes from buffers the shader indexes
    // through the bindless set, not from fixed-function vertex attributes.
    VkPipelineVertexInputStateCreateInfo vertexInput{};
    vertexInput.sType = VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO;

    VkPipelineInputAssemblyStateCreateInfo inputAssembly{};
    inputAssembly.sType    = VK_STRUCTURE_TYPE_PIPELINE_INPUT_ASSEMBLY_STATE_CREATE_INFO;
    inputAssembly.topology = desc.topology;

    VkPipelineViewportStateCreateInfo viewport{};
    viewport.sType         = VK_STRUCTURE_TYPE_PIPELINE_VIEWPORT_STATE_CREATE_INFO;
    viewport.viewportCount = 1;
    viewport.scissorCount  = 1;

    VkPipelineRasterizationStateCreateInfo raster{};
    raster.sType       = VK_STRUCTURE_TYPE_PIPELINE_RASTERIZATION_STATE_CREATE_INFO;
    raster.polygonMode = VK_POLYGON_MODE_FILL;
    raster.cullMode    = desc.cullMode;
    raster.frontFace   = desc.frontFace;
    raster.lineWidth   = 1.0f;

    VkPipelineMultisampleStateCreateInfo multisample{};
    multisample.sType                = VK_STRUCTURE_TYPE_PIPELINE_MULTISAMPLE_STATE_CREATE_INFO;
    multisample.rasterizationSamples = VK_SAMPLE_COUNT_1_BIT;

    VkPipelineDepthStencilStateCreateInfo depthStencil{};
    depthStencil.sType            = VK_STRUCTURE_TYPE_PIPELINE_DEPTH_STENCIL_STATE_CREATE_INFO;
    depthStencil.depthTestEnable  = desc.depthTest ? VK_TRUE : VK_FALSE;
    depthStencil.depthWriteEnable = desc.depthWrite ? VK_TRUE : VK_FALSE;
    depthStencil.depthCompareOp   = VK_COMPARE_OP_GREATER_OR_EQUAL; // reverse-Z
    depthStencil.maxDepthBounds   = 1.0f;

    std::vector<VkPipelineColorBlendAttachmentState> blendAttachments(desc.colorFormats.size());
    for (VkPipelineColorBlendAttachmentState& attachment : blendAttachments) {
        attachment.colorWriteMask = VK_COLOR_COMPONENT_R_BIT | VK_COLOR_COMPONENT_G_BIT
                                  | VK_COLOR_COMPONENT_B_BIT | VK_COLOR_COMPONENT_A_BIT;
        attachment.blendEnable = desc.blend ? VK_TRUE : VK_FALSE;
        if (desc.blend) {
            attachment.srcColorBlendFactor = VK_BLEND_FACTOR_SRC_ALPHA;
            attachment.dstColorBlendFactor = VK_BLEND_FACTOR_ONE_MINUS_SRC_ALPHA;
            attachment.colorBlendOp        = VK_BLEND_OP_ADD;
            attachment.srcAlphaBlendFactor = VK_BLEND_FACTOR_ONE;
            attachment.dstAlphaBlendFactor = VK_BLEND_FACTOR_ZERO;
            attachment.alphaBlendOp        = VK_BLEND_OP_ADD;
        }
    }

    VkPipelineColorBlendStateCreateInfo blend{};
    blend.sType           = VK_STRUCTURE_TYPE_PIPELINE_COLOR_BLEND_STATE_CREATE_INFO;
    blend.attachmentCount = static_cast<std::uint32_t>(blendAttachments.size());
    blend.pAttachments    = blendAttachments.empty() ? nullptr : blendAttachments.data();

    const VkDynamicState dynamicStates[] = {VK_DYNAMIC_STATE_VIEWPORT, VK_DYNAMIC_STATE_SCISSOR};
    VkPipelineDynamicStateCreateInfo dynamic{};
    dynamic.sType             = VK_STRUCTURE_TYPE_PIPELINE_DYNAMIC_STATE_CREATE_INFO;
    dynamic.dynamicStateCount = 2;
    dynamic.pDynamicStates    = dynamicStates;

    VkPushConstantRange pushRange{};
    pushRange.stageFlags = VK_SHADER_STAGE_ALL;
    pushRange.size       = desc.pushConstantBytes;

    VkPipelineLayoutCreateInfo layoutInfo{};
    layoutInfo.sType = VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO;
    if (desc.descriptorSetLayout != VK_NULL_HANDLE) {
        layoutInfo.setLayoutCount = 1;
        layoutInfo.pSetLayouts    = &desc.descriptorSetLayout;
    }
    if (desc.pushConstantBytes > 0) {
        layoutInfo.pushConstantRangeCount = 1;
        layoutInfo.pPushConstantRanges    = &pushRange;
    }
    HARPIA_VK_CHECK(vkCreatePipelineLayout(handle, &layoutInfo, nullptr, &layout_));

    // Dynamic rendering: the pipeline states its attachment formats instead of
    // referencing a render pass.
    VkPipelineRenderingCreateInfo renderingInfo{};
    renderingInfo.sType = VK_STRUCTURE_TYPE_PIPELINE_RENDERING_CREATE_INFO;
    renderingInfo.colorAttachmentCount    = static_cast<std::uint32_t>(desc.colorFormats.size());
    renderingInfo.pColorAttachmentFormats = desc.colorFormats.empty()
                                          ? nullptr : desc.colorFormats.data();
    renderingInfo.depthAttachmentFormat   = desc.depthFormat;

    VkGraphicsPipelineCreateInfo info{};
    info.sType               = VK_STRUCTURE_TYPE_GRAPHICS_PIPELINE_CREATE_INFO;
    info.pNext               = &renderingInfo;
    info.stageCount          = 2;
    info.pStages             = stages;
    info.pVertexInputState   = &vertexInput;
    info.pInputAssemblyState = &inputAssembly;
    info.pViewportState      = &viewport;
    info.pRasterizationState = &raster;
    info.pMultisampleState   = &multisample;
    info.pDepthStencilState  = &depthStencil;
    info.pColorBlendState    = &blend;
    info.pDynamicState       = &dynamic;
    info.layout              = layout_;

    const VkResult result = vkCreateGraphicsPipelines(handle, VK_NULL_HANDLE, 1, &info,
                                                      nullptr, &pipeline_);

    vkDestroyShaderModule(handle, vertexModule, nullptr);
    vkDestroyShaderModule(handle, fragmentModule, nullptr);

    if (result != VK_SUCCESS) {
        std::fprintf(stderr, "[vulkan] vkCreateGraphicsPipelines failed: %s\n",
                     resultToString(result));
        return false;
    }

    setDebugName(handle, VK_OBJECT_TYPE_PIPELINE, pipeline_, desc.debugName);
    setDebugName(handle, VK_OBJECT_TYPE_PIPELINE_LAYOUT, layout_, desc.debugName);
    return true;
}

void VulkanPipeline::destroy()
{
    if (device_ == nullptr) {
        return;
    }
    if (pipeline_ != VK_NULL_HANDLE) {
        vkDestroyPipeline(device_->device(), pipeline_, nullptr);
        pipeline_ = VK_NULL_HANDLE;
    }
    if (layout_ != VK_NULL_HANDLE) {
        vkDestroyPipelineLayout(device_->device(), layout_, nullptr);
        layout_ = VK_NULL_HANDLE;
    }
    device_ = nullptr;
}

void VulkanPipeline::bind(VkCommandBuffer cmd) const
{
    vkCmdBindPipeline(cmd, VK_PIPELINE_BIND_POINT_GRAPHICS, pipeline_);
}

void VulkanPipeline::setViewportAndScissor(VkCommandBuffer cmd, VkExtent2D extent) const
{
    VkViewport viewport{};
    viewport.x        = 0.0f;
    viewport.y        = 0.0f;
    viewport.width    = static_cast<float>(extent.width);
    viewport.height   = static_cast<float>(extent.height);
    viewport.minDepth = 0.0f;
    viewport.maxDepth = 1.0f;
    vkCmdSetViewport(cmd, 0, 1, &viewport);

    VkRect2D scissor{};
    scissor.extent = extent;
    vkCmdSetScissor(cmd, 0, 1, &scissor);
}

// --- compute ----------------------------------------------------------------

VulkanComputePipeline::~VulkanComputePipeline()
{
    destroy();
}

bool VulkanComputePipeline::create(VulkanDevice& device, const ComputePipelineDesc& desc)
{
    device_ = &device;
    const VkDevice handle = device.device();

    const std::vector<std::uint32_t> code = loadSpirv(desc.spirvPath);
    if (code.empty()) {
        return false;
    }

    VkShaderModule module = createModule(handle, code, "Shader_Compute");
    if (module == VK_NULL_HANDLE) {
        return false;
    }

    VkPushConstantRange pushRange{};
    pushRange.stageFlags = VK_SHADER_STAGE_ALL;
    pushRange.size       = desc.pushConstantBytes;

    VkPipelineLayoutCreateInfo layoutInfo{};
    layoutInfo.sType = VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO;
    if (desc.descriptorSetLayout != VK_NULL_HANDLE) {
        layoutInfo.setLayoutCount = 1;
        layoutInfo.pSetLayouts    = &desc.descriptorSetLayout;
    }
    if (desc.pushConstantBytes > 0) {
        layoutInfo.pushConstantRangeCount = 1;
        layoutInfo.pPushConstantRanges    = &pushRange;
    }
    HARPIA_VK_CHECK(vkCreatePipelineLayout(handle, &layoutInfo, nullptr, &layout_));

    VkComputePipelineCreateInfo info{};
    info.sType        = VK_STRUCTURE_TYPE_COMPUTE_PIPELINE_CREATE_INFO;
    info.stage.sType  = VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO;
    info.stage.stage  = VK_SHADER_STAGE_COMPUTE_BIT;
    info.stage.module = module;
    info.stage.pName  = "main";
    info.layout       = layout_;

    const VkResult result =
        vkCreateComputePipelines(handle, VK_NULL_HANDLE, 1, &info, nullptr, &pipeline_);

    // The module is only needed while the pipeline is being built.
    vkDestroyShaderModule(handle, module, nullptr);

    if (result != VK_SUCCESS) {
        std::fprintf(stderr, "[vulkan] vkCreateComputePipelines failed: %s\n",
                     resultToString(result));
        destroy();
        return false;
    }

    setDebugName(handle, VK_OBJECT_TYPE_PIPELINE, pipeline_, desc.debugName);
    setDebugName(handle, VK_OBJECT_TYPE_PIPELINE_LAYOUT, layout_, desc.debugName);
    return true;
}

void VulkanComputePipeline::destroy()
{
    if (device_ == nullptr) {
        return;
    }
    if (pipeline_ != VK_NULL_HANDLE) {
        vkDestroyPipeline(device_->device(), pipeline_, nullptr);
        pipeline_ = VK_NULL_HANDLE;
    }
    if (layout_ != VK_NULL_HANDLE) {
        vkDestroyPipelineLayout(device_->device(), layout_, nullptr);
        layout_ = VK_NULL_HANDLE;
    }
    device_ = nullptr;
}

void VulkanComputePipeline::bind(VkCommandBuffer cmd) const
{
    vkCmdBindPipeline(cmd, VK_PIPELINE_BIND_POINT_COMPUTE, pipeline_);
}

void VulkanComputePipeline::dispatchCovering(VkCommandBuffer cmd,
                                             std::uint32_t   width,
                                             std::uint32_t   height,
                                             std::uint32_t   depth,
                                             std::uint32_t   groupWidth,
                                             std::uint32_t   groupHeight,
                                             std::uint32_t   groupDepth)
{
    const auto groups = [](std::uint32_t total, std::uint32_t size) {
        return size == 0 ? 0u : (total + size - 1) / size;
    };
    vkCmdDispatch(cmd,
                  groups(width, groupWidth),
                  groups(height, groupHeight),
                  groups(depth, groupDepth));
}

} // namespace harpia::rhi
