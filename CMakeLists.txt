cmake_minimum_required(VERSION 3.28)

project(HarpiaEngine
    VERSION 0.1.0
    DESCRIPTION "Harpia Engine"
    LANGUAGES CXX)

# ------------------------------------------------------------------------------
# Options
# ------------------------------------------------------------------------------
option(HARPIA_BUILD_TESTS  "Build unit tests"                ON)
option(HARPIA_ENABLE_TRACY "Enable Tracy profiler"           OFF)
option(HARPIA_WERROR       "Treat warnings as errors"        OFF)

# ------------------------------------------------------------------------------
# Global settings
# ------------------------------------------------------------------------------
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

set(CMAKE_ARCHIVE_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/lib)
set(CMAKE_LIBRARY_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/lib)
set(CMAKE_RUNTIME_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/bin)

if(NOT CMAKE_BUILD_TYPE AND NOT CMAKE_CONFIGURATION_TYPES)
    set(CMAKE_BUILD_TYPE Debug CACHE STRING "" FORCE)
endif()

set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

# ------------------------------------------------------------------------------
# Warning interface — every Harpia target links this
# ------------------------------------------------------------------------------
add_library(harpia_warnings INTERFACE)
target_compile_options(harpia_warnings INTERFACE
    -Wall
    -Wextra
    -Wpedantic
    -Wshadow
    -Wnon-virtual-dtor
    -Wcast-align
    -Wunused
    -Woverloaded-virtual
    -Wconversion
    -Wsign-conversion
    -Wdouble-promotion
    -Wformat=2)

if(HARPIA_WERROR)
    target_compile_options(harpia_warnings INTERFACE -Werror)
endif()

# ------------------------------------------------------------------------------
# Dependencies
# ------------------------------------------------------------------------------
list(APPEND CMAKE_MODULE_PATH ${CMAKE_CURRENT_SOURCE_DIR}/cmake)

include(FetchContent)
set(FETCHCONTENT_QUIET OFF)

find_package(Vulkan REQUIRED)
find_package(glfw3 3.3 REQUIRED)

FetchContent_Declare(vma
    GIT_REPOSITORY https://github.com/GPUOpen-LibrariesAndSDKs/VulkanMemoryAllocator.git
    GIT_TAG        v3.4.0
    GIT_SHALLOW    TRUE)
FetchContent_MakeAvailable(vma)

# stb ships headers only, with no build system of its own.
FetchContent_Declare(stb
    GIT_REPOSITORY https://github.com/nothings/stb.git
    GIT_TAG        f0569113c93ad095470c54bf34a17b36646bbbb5
    GIT_SHALLOW    FALSE)
FetchContent_MakeAvailable(stb)

FetchContent_Declare(glm
    GIT_REPOSITORY https://github.com/g-truc/glm.git
    GIT_TAG        1.0.1
    GIT_SHALLOW    TRUE)
set(GLM_BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(GLM_BUILD_LIBRARY OFF CACHE BOOL "" FORCE)
FetchContent_MakeAvailable(glm)

# Wrapped rather than used directly, the same way stb and cgltf are: glm's own
# target carries its includes through a generator expression, which defeats
# promoting them to SYSTEM after the fact. Its headers trip -Wsign-conversion
# and are not ours to fix.
add_library(harpia_glm INTERFACE)
target_include_directories(harpia_glm SYSTEM INTERFACE ${glm_SOURCE_DIR})

# Set once, globally: a translation unit that includes glm without these would
# disagree with every other one about depth range and constructor behaviour.
target_compile_definitions(harpia_glm INTERFACE
    GLM_FORCE_DEPTH_ZERO_TO_ONE   # Vulkan clip depth is [0,1], not [-1,1]
    GLM_FORCE_CTOR_INIT           # default-construct to zero, not to garbage
    GLM_ENABLE_EXPERIMENTAL)

FetchContent_Declare(cgltf
    GIT_REPOSITORY https://github.com/jkuhlmann/cgltf.git
    GIT_TAG        v1.15
    GIT_SHALLOW    TRUE)
FetchContent_MakeAvailable(cgltf)

add_library(cgltf INTERFACE)
target_include_directories(cgltf SYSTEM INTERFACE ${cgltf_SOURCE_DIR})

add_library(stb INTERFACE)
target_include_directories(stb SYSTEM INTERFACE ${stb_SOURCE_DIR})

if(HARPIA_ENABLE_TRACY)
    FetchContent_Declare(tracy
        GIT_REPOSITORY https://github.com/wolfpld/tracy.git
        GIT_TAG        v0.11.1
        GIT_SHALLOW    TRUE)
    set(TRACY_ENABLE ON  CACHE BOOL "" FORCE)
    set(TRACY_ON_DEMAND ON CACHE BOOL "" FORCE)
    FetchContent_MakeAvailable(tracy)
endif()

# ------------------------------------------------------------------------------
# Targets
# ------------------------------------------------------------------------------
add_subdirectory(Source/Core)
add_subdirectory(Source/Platform)
add_subdirectory(Source/RHI)
include(HarpiaShaders)

harpia_declare_shaders(SOURCES
    ${CMAKE_CURRENT_SOURCE_DIR}/Shaders/Triangle.vert.hlsl
    ${CMAKE_CURRENT_SOURCE_DIR}/Shaders/Triangle.frag.hlsl
    ${CMAKE_CURRENT_SOURCE_DIR}/Shaders/GBuffer.vert.hlsl
    ${CMAKE_CURRENT_SOURCE_DIR}/Shaders/GBuffer.frag.hlsl
    ${CMAKE_CURRENT_SOURCE_DIR}/Shaders/Fullscreen.vert.hlsl
    ${CMAKE_CURRENT_SOURCE_DIR}/Shaders/Lighting.frag.hlsl)

add_subdirectory(Samples/ClearScreen)
add_subdirectory(Samples/Triangle)

if(HARPIA_BUILD_TESTS)
    enable_testing()
    add_subdirectory(Tests)
endif()

# ------------------------------------------------------------------------------
message(STATUS "")
message(STATUS "Harpia ${PROJECT_VERSION}")
message(STATUS "  build type : ${CMAKE_BUILD_TYPE}")
message(STATUS "  compiler   : ${CMAKE_CXX_COMPILER_ID} ${CMAKE_CXX_COMPILER_VERSION}")
message(STATUS "  tests      : ${HARPIA_BUILD_TESTS}")
message(STATUS "  tracy      : ${HARPIA_ENABLE_TRACY}")
message(STATUS "  werror     : ${HARPIA_WERROR}")
message(STATUS "")
