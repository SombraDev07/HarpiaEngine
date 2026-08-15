add_executable(HarpiaDeferred main.cpp)

target_link_libraries(HarpiaDeferred
    PRIVATE Harpia::RHI
            Harpia::Platform
            Harpia::Core
            harpia_warnings)

harpia_use_shaders(HarpiaDeferred)
