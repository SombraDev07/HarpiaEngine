#include "RHI/RenderGraph/RenderGraph.h"

#include "RHI/Vulkan/VulkanDevice.h"

#include <vk_mem_alloc.h>

#include <algorithm>
#include <cassert>

namespace harpia::rhi {
namespace {

struct UsageTraits {
    VkImageLayout         layout;
    VkPipelineStageFlags2 stage;
    VkAccessFlags2        access;
};

// The single table every barrier in the engine is derived from. Adding a usage
// means adding one row here, not hunting for barrier sites.
[[nodiscard]] UsageTraits traitsOf(RgUsage usage) noexcept
{
    switch (usage) {
        case RgUsage::ColorAttachment:
            return {VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
                    VK_PIPELINE_STAGE_2_COLOR_ATTACHMENT_OUTPUT_BIT,
                    VK_ACCESS_2_COLOR_ATTACHMENT_WRITE_BIT
                        | VK_ACCESS_2_COLOR_ATTACHMENT_READ_BIT};
        case RgUsage::DepthAttachment:
            return {VK_IMAGE_LAYOUT_DEPTH_ATTACHMENT_OPTIMAL,
                    VK_PIPELINE_STAGE_2_EARLY_FRAGMENT_TESTS_BIT
                        | VK_PIPELINE_STAGE_2_LATE_FRAGMENT_TESTS_BIT,
                    VK_ACCESS_2_DEPTH_STENCIL_ATTACHMENT_WRITE_BIT
                        | VK_ACCESS_2_DEPTH_STENCIL_ATTACHMENT_READ_BIT};
        case RgUsage::SampledRead:
            return {VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
                    VK_PIPELINE_STAGE_2_FRAGMENT_SHADER_BIT
                        | VK_PIPELINE_STAGE_2_COMPUTE_SHADER_BIT,
                    VK_ACCESS_2_SHADER_SAMPLED_READ_BIT};
        case RgUsage::StorageRead:
            return {VK_IMAGE_LAYOUT_GENERAL,
                    VK_PIPELINE_STAGE_2_COMPUTE_SHADER_BIT,
                    VK_ACCESS_2_SHADER_STORAGE_READ_BIT};
        case RgUsage::StorageWrite:
            return {VK_IMAGE_LAYOUT_GENERAL,
                    VK_PIPELINE_STAGE_2_COMPUTE_SHADER_BIT,
                    VK_ACCESS_2_SHADER_STORAGE_WRITE_BIT
                        | VK_ACCESS_2_SHADER_STORAGE_READ_BIT};
        case RgUsage::TransferSrc:
            return {VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                    VK_PIPELINE_STAGE_2_COPY_BIT,
                    VK_ACCESS_2_TRANSFER_READ_BIT};
        case RgUsage::TransferDst:
            return {VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                    VK_PIPELINE_STAGE_2_COPY_BIT,
                    VK_ACCESS_2_TRANSFER_WRITE_BIT};
        case RgUsage::Present:
            return {VK_IMAGE_LAYOUT_PRESENT_SRC_KHR,
                    VK_PIPELINE_STAGE_2_BOTTOM_OF_PIPE_BIT,
                    0};
    }
    return {VK_IMAGE_LAYOUT_GENERAL, VK_PIPELINE_STAGE_2_ALL_COMMANDS_BIT, 0};
}

[[nodiscard]] VkImageUsageFlags usageFlagsFor(const RgTextureDesc& desc) noexcept
{
    // Transients are given the union of what a graph pass might do with them.
    // Narrowing this needs the full pass list, which compile() has but
    // creation does not; the cost is a few driver-side capability bits.
    return desc.extraUsage
         | VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT
         | VK_IMAGE_USAGE_SAMPLED_BIT
         | VK_IMAGE_USAGE_TRANSFER_SRC_BIT
         | VK_IMAGE_USAGE_TRANSFER_DST_BIT;
}

[[nodiscard]] bool isDepthFormat(VkFormat format) noexcept
{
    switch (format) {
        case VK_FORMAT_D16_UNORM:
        case VK_FORMAT_D32_SFLOAT:
        case VK_FORMAT_D24_UNORM_S8_UINT:
        case VK_FORMAT_D32_SFLOAT_S8_UINT:
            return true;
        default:
            return false;
    }
}

} // namespace

// --- RgBuilder --------------------------------------------------------------

RgHandle RgBuilder::createTexture(const char* name, const RgTextureDesc& desc)
{
    RenderGraph::Resource resource;
    resource.name     = name;
    resource.desc     = desc;
    resource.imported = false;
    return graph_.addResource(std::move(resource));
}

void RgBuilder::read(RgHandle handle, RgUsage usage)
{
    graph_.recordAccess(passIndex_, RenderGraph::Access{handle, usage,
                                                        VK_ATTACHMENT_LOAD_OP_LOAD, {}},
                        false);
}

void RgBuilder::write(RgHandle handle, RgUsage usage)
{
    graph_.recordAccess(passIndex_, RenderGraph::Access{handle, usage,
                                                        VK_ATTACHMENT_LOAD_OP_LOAD, {}},
                        true);
}

void RgBuilder::writeColor(RgHandle handle, VkAttachmentLoadOp loadOp, VkClearValue clearValue)
{
    graph_.recordAccess(passIndex_,
                        RenderGraph::Access{handle, RgUsage::ColorAttachment, loadOp, clearValue},
                        true);
}

void RgBuilder::writeDepth(RgHandle handle, VkAttachmentLoadOp loadOp, VkClearValue clearValue)
{
    graph_.recordAccess(passIndex_,
                        RenderGraph::Access{handle, RgUsage::DepthAttachment, loadOp, clearValue},
                        true);
}

void RgBuilder::neverCull()
{
    graph_.markNeverCull(passIndex_);
}

// --- RgContext --------------------------------------------------------------

VkImage RgContext::image(RgHandle handle) const
{
    const RenderGraph::Resource& resource = graph_.resourceOf(handle);
    return resource.imported ? resource.importedImage
                             : graph_.physical_[resource.physical].image;
}

VkImageView RgContext::view(RgHandle handle) const
{
    const RenderGraph::Resource& resource = graph_.resourceOf(handle);
    return resource.imported ? resource.importedView
                             : graph_.physical_[resource.physical].view;
}

VkExtent2D RgContext::extent(RgHandle handle) const
{
    const RenderGraph::Resource& resource = graph_.resourceOf(handle);
    return VkExtent2D{resource.desc.width, resource.desc.height};
}

// --- RenderGraph ------------------------------------------------------------

RenderGraph::~RenderGraph()
{
    destroy();
}

bool RenderGraph::create(VulkanDevice& device)
{
    device_ = &device;
    return true;
}

void RenderGraph::destroy()
{
    if (device_ == nullptr) {
        return;
    }
    vkDeviceWaitIdle(device_->device());

    for (PhysicalImage& physical : physical_) {
        if (physical.view != VK_NULL_HANDLE) {
            vkDestroyImageView(device_->device(), physical.view, nullptr);
        }
        if (physical.image != VK_NULL_HANDLE) {
            vmaDestroyImage(device_->allocator(), physical.image, physical.allocation);
        }
    }
    physical_.clear();
    passes_.clear();
    resources_.clear();
    order_.clear();
    device_ = nullptr;
}

void RenderGraph::beginFrame()
{
    passes_.clear();
    resources_.clear();
    order_.clear();
    stats_ = Stats{};
    compiled_ = false;

    // Physical images survive the frame boundary; only their assignment resets.
    for (PhysicalImage& physical : physical_) {
        physical.inUse   = false;
        physical.lastUse = 0;
    }
}

RgHandle RenderGraph::importTexture(const char*   name,
                                    VkImage       image,
                                    VkImageView   view,
                                    VkFormat      format,
                                    VkExtent2D    extent,
                                    VkImageLayout currentLayout)
{
    Resource resource;
    resource.name          = name;
    resource.imported      = true;
    resource.importedImage = image;
    resource.importedView  = view;
    resource.desc.width    = extent.width;
    resource.desc.height   = extent.height;
    resource.desc.format   = format;
    resource.layout        = currentLayout;
    return addResource(std::move(resource));
}

RgHandle RenderGraph::addResource(Resource&& resource)
{
    resources_.push_back(std::move(resource));
    return static_cast<RgHandle>(resources_.size() - 1);
}

void RenderGraph::addPass(const char* name, SetupFn setup, ExecuteFn execute)
{
    Pass pass;
    pass.name    = name;
    pass.setup   = std::move(setup);
    pass.execute = std::move(execute);
    passes_.push_back(std::move(pass));

    const auto index = static_cast<std::uint32_t>(passes_.size() - 1);
    RgBuilder builder(*this, index);
    passes_[index].setup(builder);
}

void RenderGraph::recordAccess(std::uint32_t passIndex, const Access& access, bool isWrite)
{
    if (access.handle >= resources_.size()) {
        assert(false && "render graph access to an unknown resource");
        return;
    }
    if (isWrite) {
        passes_[passIndex].writes.push_back(access);
    } else {
        passes_[passIndex].reads.push_back(access);
        resources_[access.handle].readByAnyone = true;
    }
}

void RenderGraph::markNeverCull(std::uint32_t passIndex)
{
    passes_[passIndex].neverCull = true;
}

void RenderGraph::compile()
{
    if (compiled_) {
        return;
    }

    // --- cull ---------------------------------------------------------------
    // A pass survives if it was pinned, if it writes an imported resource
    // (something outside the graph consumes it), or if a surviving pass reads
    // something it writes. Iterating backwards settles that in one sweep.
    std::vector<bool> resourceNeeded(resources_.size(), false);
    for (std::size_t i = 0; i < resources_.size(); ++i) {
        if (resources_[i].imported) {
            resourceNeeded[i] = true;
        }
    }

    for (std::size_t i = passes_.size(); i-- > 0;) {
        Pass& pass = passes_[i];

        bool needed = pass.neverCull;
        if (!needed) {
            for (const Access& write : pass.writes) {
                if (resourceNeeded[write.handle]) {
                    needed = true;
                    break;
                }
            }
        }

        pass.culled = !needed;
        if (needed) {
            for (const Access& read : pass.reads) {
                resourceNeeded[read.handle] = true;
            }
        }
    }

    order_.clear();
    for (std::uint32_t i = 0; i < passes_.size(); ++i) {
        if (!passes_[i].culled) {
            order_.push_back(i);
        } else {
            ++stats_.culledPasses;
        }
    }
    stats_.passes = static_cast<std::uint32_t>(order_.size());

    // --- lifetimes ----------------------------------------------------------
    for (std::uint32_t slot = 0; slot < order_.size(); ++slot) {
        const Pass& pass = passes_[order_[slot]];
        const auto touch = [&](RgHandle handle) {
            Resource& resource = resources_[handle];
            if (resource.firstUse == kRgInvalid) {
                resource.firstUse = slot;
            }
            resource.lastUse = slot;
        };
        for (const Access& access : pass.reads)  { touch(access.handle); }
        for (const Access& access : pass.writes) { touch(access.handle); }
    }

    // --- assign physical images --------------------------------------------
    for (Resource& resource : resources_) {
        if (resource.imported || resource.firstUse == kRgInvalid) {
            continue; // imported, or belongs to a culled pass
        }
        ++stats_.transients;
        resource.physical = acquirePhysical(resource.desc, resource.firstUse);
        physical_[resource.physical].lastUse = resource.lastUse;
    }

    stats_.physicalImages = static_cast<std::uint32_t>(physical_.size());
    compiled_ = true;
}

std::uint32_t RenderGraph::acquirePhysical(const RgTextureDesc& desc, std::uint32_t firstUse)
{
    // Aliasing: a transient can take over an image whose previous user has
    // already finished. Resource-level rather than memory-level, so it is safe
    // without manual VMA block juggling, and it still collapses a chain of
    // same-sized intermediates onto one allocation.
    for (std::uint32_t i = 0; i < physical_.size(); ++i) {
        PhysicalImage& physical = physical_[i];
        if (physical.desc == desc && physical.inUse && physical.lastUse < firstUse) {
            ++stats_.aliasedImages;
            return i;
        }
    }
    for (std::uint32_t i = 0; i < physical_.size(); ++i) {
        PhysicalImage& physical = physical_[i];
        if (physical.desc == desc && !physical.inUse) {
            physical.inUse = true;
            return i;
        }
    }

    PhysicalImage physical;
    physical.desc  = desc;
    physical.inUse = true;

    VkImageCreateInfo imageInfo{};
    imageInfo.sType       = VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO;
    imageInfo.imageType   = VK_IMAGE_TYPE_2D;
    imageInfo.format      = desc.format;
    imageInfo.extent      = VkExtent3D{desc.width, desc.height, 1};
    imageInfo.mipLevels   = 1;
    imageInfo.arrayLayers = 1;
    imageInfo.samples     = VK_SAMPLE_COUNT_1_BIT;
    imageInfo.tiling      = VK_IMAGE_TILING_OPTIMAL;
    imageInfo.usage       = isDepthFormat(desc.format)
                          ? (VK_IMAGE_USAGE_DEPTH_STENCIL_ATTACHMENT_BIT
                             | VK_IMAGE_USAGE_SAMPLED_BIT)
                          : usageFlagsFor(desc);
    imageInfo.initialLayout = VK_IMAGE_LAYOUT_UNDEFINED;

    VmaAllocationCreateInfo allocInfo{};
    allocInfo.usage = VMA_MEMORY_USAGE_AUTO;

    HARPIA_VK_CHECK(vmaCreateImage(device_->allocator(), &imageInfo, &allocInfo,
                                   &physical.image, &physical.allocation, nullptr));

    VkImageViewCreateInfo viewInfo{};
    viewInfo.sType    = VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO;
    viewInfo.image    = physical.image;
    viewInfo.viewType = VK_IMAGE_VIEW_TYPE_2D;
    viewInfo.format   = desc.format;
    viewInfo.subresourceRange.aspectMask = isDepthFormat(desc.format)
                                         ? VK_IMAGE_ASPECT_DEPTH_BIT
                                         : VK_IMAGE_ASPECT_COLOR_BIT;
    viewInfo.subresourceRange.levelCount = 1;
    viewInfo.subresourceRange.layerCount = 1;
    HARPIA_VK_CHECK(vkCreateImageView(device_->device(), &viewInfo, nullptr, &physical.view));

    physical_.push_back(physical);
    const auto index = static_cast<std::uint32_t>(physical_.size() - 1);

    char name[64];
    std::snprintf(name, sizeof(name), "RG_Physical_%u", index);
    setDebugName(device_->device(), VK_OBJECT_TYPE_IMAGE, physical.image, name);

    return index;
}

void RenderGraph::barrierTo(VkCommandBuffer cmd, Resource& resource, RgUsage usage)
{
    const UsageTraits traits = traitsOf(usage);

    const bool layoutChanges = resource.layout != traits.layout;
    const bool writeInvolved =
        (resource.access & (VK_ACCESS_2_COLOR_ATTACHMENT_WRITE_BIT
                          | VK_ACCESS_2_DEPTH_STENCIL_ATTACHMENT_WRITE_BIT
                          | VK_ACCESS_2_SHADER_STORAGE_WRITE_BIT
                          | VK_ACCESS_2_TRANSFER_WRITE_BIT)) != 0
        || (traits.access & (VK_ACCESS_2_COLOR_ATTACHMENT_WRITE_BIT
                           | VK_ACCESS_2_DEPTH_STENCIL_ATTACHMENT_WRITE_BIT
                           | VK_ACCESS_2_SHADER_STORAGE_WRITE_BIT
                           | VK_ACCESS_2_TRANSFER_WRITE_BIT)) != 0;

    // Read-after-read with the same layout needs no barrier; emitting one
    // anyway is exactly the invisible stall this table exists to avoid.
    if (!layoutChanges && !writeInvolved) {
        return;
    }

    const VkImage image = resource.imported
                        ? resource.importedImage
                        : physical_[resource.physical].image;

    imageBarrier(cmd, image,
                 resource.layout, traits.layout,
                 resource.stage, resource.access,
                 traits.stage, traits.access,
                 isDepthFormat(resource.desc.format) ? VK_IMAGE_ASPECT_DEPTH_BIT
                                                     : VK_IMAGE_ASPECT_COLOR_BIT);

    resource.layout = traits.layout;
    resource.stage  = traits.stage;
    resource.access = traits.access;
    ++stats_.barriers;
}

void RenderGraph::execute(VkCommandBuffer cmd)
{
    if (!compiled_) {
        compile();
    }

    for (const std::uint32_t passIndex : order_) {
        Pass& pass = passes_[passIndex];

        DebugLabel label(cmd, pass.name.c_str(), 0.3f, 0.6f, 0.9f);

        for (const Access& access : pass.reads) {
            barrierTo(cmd, resources_[access.handle], access.usage);
        }
        for (const Access& access : pass.writes) {
            barrierTo(cmd, resources_[access.handle], access.usage);
        }

        // Gather attachments so the graph can drive dynamic rendering itself;
        // a pass never writes a VkRenderingInfo by hand.
        std::vector<VkRenderingAttachmentInfo> colorAttachments;
        VkRenderingAttachmentInfo              depthAttachment{};
        bool                                   hasDepth = false;
        VkExtent2D                             area{};

        for (const Access& access : pass.writes) {
            const Resource& resource = resources_[access.handle];
            const VkImageView view = resource.imported
                                   ? resource.importedView
                                   : physical_[resource.physical].view;

            if (access.usage == RgUsage::ColorAttachment) {
                VkRenderingAttachmentInfo attachment{};
                attachment.sType       = VK_STRUCTURE_TYPE_RENDERING_ATTACHMENT_INFO;
                attachment.imageView   = view;
                attachment.imageLayout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL;
                attachment.loadOp      = access.loadOp;
                attachment.storeOp     = VK_ATTACHMENT_STORE_OP_STORE;
                attachment.clearValue  = access.clearValue;
                colorAttachments.push_back(attachment);
                area = VkExtent2D{resource.desc.width, resource.desc.height};
            } else if (access.usage == RgUsage::DepthAttachment) {
                depthAttachment.sType       = VK_STRUCTURE_TYPE_RENDERING_ATTACHMENT_INFO;
                depthAttachment.imageView   = view;
                depthAttachment.imageLayout = VK_IMAGE_LAYOUT_DEPTH_ATTACHMENT_OPTIMAL;
                depthAttachment.loadOp      = access.loadOp;
                depthAttachment.storeOp     = VK_ATTACHMENT_STORE_OP_STORE;
                depthAttachment.clearValue  = access.clearValue;
                hasDepth = true;
                area = VkExtent2D{resource.desc.width, resource.desc.height};
            }
        }

        const bool rendering = !colorAttachments.empty() || hasDepth;
        if (rendering) {
            VkRenderingInfo info{};
            info.sType                = VK_STRUCTURE_TYPE_RENDERING_INFO;
            info.renderArea.extent    = area;
            info.layerCount           = 1;
            info.colorAttachmentCount = static_cast<std::uint32_t>(colorAttachments.size());
            info.pColorAttachments    = colorAttachments.empty() ? nullptr
                                                                 : colorAttachments.data();
            info.pDepthAttachment     = hasDepth ? &depthAttachment : nullptr;
            vkCmdBeginRendering(cmd, &info);
        }

        RgContext context(*this, cmd);
        if (pass.execute) {
            pass.execute(context);
        }

        if (rendering) {
            vkCmdEndRendering(cmd);
        }
    }
}

const RenderGraph::Resource& RenderGraph::resourceOf(RgHandle handle) const
{
    assert(handle < resources_.size() && "render graph handle out of range");
    return resources_[handle];
}

VkImageLayout RenderGraph::finalLayout(RgHandle handle) const
{
    return resourceOf(handle).layout;
}

} // namespace harpia::rhi
