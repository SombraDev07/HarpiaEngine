add_executable(HarpiaTriangle main.cpp)

target_link_libraries(HarpiaTriangle
    PRIVATE Harpia::RHI
            Harpia::Platform
            Harpia::Core
            harpia_warnings)

harpia_use_shaders(HarpiaTriangle)
