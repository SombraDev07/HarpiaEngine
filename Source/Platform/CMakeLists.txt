add_library(harpia_platform STATIC
    Window.cpp
    Input.cpp)

add_library(Harpia::Platform ALIAS harpia_platform)

target_include_directories(harpia_platform PUBLIC ${PROJECT_SOURCE_DIR}/Source)

target_link_libraries(harpia_platform
    PUBLIC  Harpia::Core
            glfw
    PRIVATE harpia_warnings)

target_sources(harpia_platform PUBLIC
    FILE_SET harpia_platform_headers TYPE HEADERS
    BASE_DIRS ${PROJECT_SOURCE_DIR}/Source
    FILES
        Window.h
        Input.h
        InputTypes.h)
