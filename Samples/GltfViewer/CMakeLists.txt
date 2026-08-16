add_executable(HarpiaGltfViewer main.cpp)

target_link_libraries(HarpiaGltfViewer
    PRIVATE Harpia::RHI
            Harpia::Platform
            Harpia::Core
            harpia_warnings)

harpia_use_shaders(HarpiaGltfViewer)
