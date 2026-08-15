add_executable(HarpiaClearScreen main.cpp)

target_link_libraries(HarpiaClearScreen
    PRIVATE Harpia::RHI
            Harpia::Platform
            Harpia::Core
            harpia_warnings)
