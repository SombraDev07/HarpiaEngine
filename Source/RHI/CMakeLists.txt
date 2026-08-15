add_library(harpia_rhi STATIC
    Vulkan/VulkanCommon.cpp
    Vulkan/VulkanDevice.cpp
    Vulkan/VulkanSwapchain.cpp
    Vulkan/VulkanBindless.cpp
    Vulkan/VulkanRenderer.cpp
    Vulkan/VulkanPipeline.cpp
    Vulkan/VulkanBuffer.cpp
    GpuMesh.cpp
    RenderGraph/RenderGraph.cpp
    ThirdPartyImpl.cpp)

add_library(Harpia::RHI ALIAS harpia_rhi)

target_include_directories(harpia_rhi PUBLIC ${PROJECT_SOURCE_DIR}/Source)

target_link_libraries(harpia_rhi
    PUBLIC  Harpia::Core
            Harpia::Platform
            Vulkan::Vulkan
            GPUOpen::VulkanMemoryAllocator
            stb
    PRIVATE harpia_warnings)

# VMA and stb generate a wall of warnings that are not ours to fix.
set_source_files_properties(ThirdPartyImpl.cpp PROPERTIES
    COMPILE_OPTIONS "-w")

target_sources(harpia_rhi PUBLIC
    FILE_SET harpia_rhi_headers TYPE HEADERS
    BASE_DIRS ${PROJECT_SOURCE_DIR}/Source
    FILES
        Vulkan/VulkanCommon.h
        Vulkan/VulkanDevice.h
        Vulkan/VulkanSwapchain.h
        Vulkan/VulkanBindless.h
        Vulkan/VulkanRenderer.h
        Vulkan/VulkanPipeline.h
        Vulkan/VulkanBuffer.h
        GpuMesh.h
        RenderGraph/RenderGraph.h)
