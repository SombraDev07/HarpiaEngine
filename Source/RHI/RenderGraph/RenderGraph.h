// Harpia Engine — render graph
//
// Roadmap F1: the graph comes before the content, inverting the usual order on
// purpose. Every pass from here on declares what it reads and writes; the graph
// derives barriers, culls work nobody consumes, and reuses transient memory.
//
// Invariant 8 lives here: no barrier is ever written by hand again. A pass says
// "I sample this" and the graph works out the layout transition, stage and
// access masks. Hand-written barriers are how a renderer accumulates stalls
// nobody can find later.
#pragma once

#include "RHI/Vulkan/VulkanCommon.h"

#include <cstdint>
#include <functional>
#include <string>
#include <vector>

struct VmaAllocation_T;
using VmaAllocation = VmaAllocation_T*;

namespace harpia::rhi {

class VulkanDevice;

using RgHandle = std::uint32_t;
inline constexpr RgHandle kRgInvalid = 0xFFFFFFFFu;

// How a pass touches a resource. This is the only thing a pass declares; every
// barrier in the frame is derived from transitions between these.
enum class RgUsage : std::uint8_t {
    ColorAttachment,
    DepthAttachment,
    SampledRead,
    StorageRead,
    StorageWrite,
    TransferSrc,
    TransferDst,
    Present,
};

struct RgTextureDesc {
    std::uint32_t width  = 0;
    std::uint32_t height = 0;
    VkFormat      format = VK_FORMAT_R8G8B8A8_UNORM;
    VkImageUsageFlags extraUsage = 0;

    [[nodiscard]] bool operator==(const RgTextureDesc& other) const noexcept
    {
        return width == other.width && height == other.height
            && format == other.format && extraUsage == other.extraUsage;
    }
};

class RenderGraph;

// Handed to a pass's setup callback. Declaring is all a pass does here — no
// Vulkan call belongs in setup.
class RgBuilder {
public:
    RgBuilder(RenderGraph& graph, std::uint32_t passIndex)
        : graph_(graph), passIndex_(passIndex) {}

    // Transient: owned by the graph, may share memory with another transient
    // whose lifetime does not overlap.
    [[nodiscard]] RgHandle createTexture(const char* name, const RgTextureDesc& desc);

    void read(RgHandle handle, RgUsage usage);
    void write(RgHandle handle, RgUsage usage);

    // Colour and depth writes carry their load op so the graph can drive
    // dynamic rendering itself.
    void writeColor(RgHandle handle,
                    VkAttachmentLoadOp loadOp = VK_ATTACHMENT_LOAD_OP_LOAD,
                    VkClearValue clearValue = {});
    void writeDepth(RgHandle handle,
                    VkAttachmentLoadOp loadOp = VK_ATTACHMENT_LOAD_OP_CLEAR,
                    VkClearValue clearValue = {});

    // Marks the pass as required even if nothing reads its outputs. Use for
    // passes with side effects the graph cannot see.
    void neverCull();

private:
    RenderGraph&  graph_;
    std::uint32_t passIndex_;
};

// Handed to a pass's execute callback.
class RgContext {
public:
    RgContext(const RenderGraph& graph, VkCommandBuffer cmd)
        : graph_(graph), cmd_(cmd) {}

    [[nodiscard]] VkCommandBuffer cmd() const noexcept { return cmd_; }

    [[nodiscard]] VkImage     image(RgHandle handle) const;
    [[nodiscard]] VkImageView view(RgHandle handle) const;
    [[nodiscard]] VkExtent2D  extent(RgHandle handle) const;

private:
    const RenderGraph& graph_;
    VkCommandBuffer    cmd_;
};

class RenderGraph {
public:
    using SetupFn   = std::function<void(RgBuilder&)>;
    using ExecuteFn = std::function<void(RgContext&)>;

    RenderGraph() = default;
    ~RenderGraph();

    RenderGraph(const RenderGraph&)            = delete;
    RenderGraph& operator=(const RenderGraph&) = delete;

    [[nodiscard]] bool create(VulkanDevice& device);
    void destroy();

    // Clears the frame's passes and resources. Physical images are kept so the
    // next frame reuses them instead of reallocating.
    void beginFrame();

    // Wraps an image the graph does not own — the swapchain image, or the
    // offscreen target. `currentLayout` is where it starts this frame.
    [[nodiscard]] RgHandle importTexture(const char*   name,
                                         VkImage       image,
                                         VkImageView   view,
                                         VkFormat      format,
                                         VkExtent2D    extent,
                                         VkImageLayout currentLayout);

    void addPass(const char* name, SetupFn setup, ExecuteFn execute);

    // Culls unreachable passes, computes resource lifetimes and assigns
    // physical images. Safe to call once per frame after all passes are added.
    void compile();

    // Records barriers and pass bodies into `cmd`.
    void execute(VkCommandBuffer cmd);

    struct Stats {
        std::uint32_t passes          = 0;
        std::uint32_t culledPasses    = 0;
        std::uint32_t transients      = 0;
        std::uint32_t aliasedImages   = 0; // transients that reused an image
        std::uint32_t barriers        = 0;
        std::uint32_t physicalImages  = 0;
    };
    [[nodiscard]] const Stats& stats() const noexcept { return stats_; }

    // Layout an imported resource is left in after execute(). The caller needs
    // this to hand the image to a presentation or readback path.
    [[nodiscard]] VkImageLayout finalLayout(RgHandle handle) const;

    // Valid after compile(): a transient has a physical image only once the
    // graph has assigned one. Needed to register a target in the bindless heap
    // before the pass that samples it records.
    [[nodiscard]] VkImage     imageOf(RgHandle handle) const;
    [[nodiscard]] VkImageView viewOf(RgHandle handle) const;

private:
    friend class RgBuilder;
    friend class RgContext;

    struct Access {
        RgHandle           handle = kRgInvalid;
        RgUsage            usage  = RgUsage::SampledRead;
        VkAttachmentLoadOp loadOp = VK_ATTACHMENT_LOAD_OP_LOAD;
        VkClearValue       clearValue{};
    };

    struct Pass {
        std::string         name;
        SetupFn             setup;
        ExecuteFn           execute;
        std::vector<Access> reads;
        std::vector<Access> writes;
        bool                neverCull = false;
        bool                culled    = false;
    };

    struct Resource {
        std::string   name;
        RgTextureDesc desc;
        bool          imported = false;

        // Imported
        VkImage       importedImage = VK_NULL_HANDLE;
        VkImageView   importedView  = VK_NULL_HANDLE;

        // Transient: index into physical_
        std::uint32_t physical = kRgInvalid;

        // Lifetime in surviving-pass order
        std::uint32_t firstUse = kRgInvalid;
        std::uint32_t lastUse  = 0;

        // Barrier tracking during execute
        VkImageLayout         layout = VK_IMAGE_LAYOUT_UNDEFINED;
        VkPipelineStageFlags2 stage  = VK_PIPELINE_STAGE_2_TOP_OF_PIPE_BIT;
        VkAccessFlags2        access = 0;

        bool readByAnyone = false;
    };

    struct PhysicalImage {
        RgTextureDesc desc;
        VkImage       image = VK_NULL_HANDLE;
        VkImageView   view  = VK_NULL_HANDLE;
        VmaAllocation allocation = nullptr;
        std::uint32_t lastUse = 0;   // last surviving-pass index using it
        bool          inUse   = false;
    };

    [[nodiscard]] RgHandle addResource(Resource&& resource);
    void  recordAccess(std::uint32_t passIndex, const Access& access, bool isWrite);
    void  markNeverCull(std::uint32_t passIndex);

    [[nodiscard]] std::uint32_t acquirePhysical(const RgTextureDesc& desc,
                                                std::uint32_t        firstUse);
    void barrierTo(VkCommandBuffer cmd, Resource& resource, RgUsage usage);

    [[nodiscard]] const Resource& resourceOf(RgHandle handle) const;

    VulkanDevice* device_ = nullptr;

    std::vector<Pass>          passes_;
    std::vector<Resource>      resources_;
    std::vector<PhysicalImage> physical_;
    std::vector<std::uint32_t> order_;   // surviving pass indices, in order

    Stats stats_;
    bool  compiled_ = false;
};

} // namespace harpia::rhi
