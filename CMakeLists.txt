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

include(HarpiaDependencies)

# ------------------------------------------------------------------------------
# Targets
# ------------------------------------------------------------------------------
add_subdirectory(Source/Core)
add_subdirectory(Source/Platform)
add_subdirectory(Source/RHI)
add_subdirectory(Source/Navigation)
include(HarpiaShaders)

harpia_declare_shaders(SOURCES
    ${CMAKE_CURRENT_SOURCE_DIR}/Shaders/Triangle.vert.hlsl
    ${CMAKE_CURRENT_SOURCE_DIR}/Shaders/Triangle.frag.hlsl
    ${CMAKE_CURRENT_SOURCE_DIR}/Shaders/GBuffer.vert.hlsl
    ${CMAKE_CURRENT_SOURCE_DIR}/Shaders/GBuffer.frag.hlsl
    ${CMAKE_CURRENT_SOURCE_DIR}/Shaders/Fullscreen.vert.hlsl
    ${CMAKE_CURRENT_SOURCE_DIR}/Shaders/Lighting.frag.hlsl
    ${CMAKE_CURRENT_SOURCE_DIR}/Shaders/Tonemap.frag.hlsl
    ${CMAKE_CURRENT_SOURCE_DIR}/Shaders/BrdfLut.frag.hlsl)

add_subdirectory(Samples/ClearScreen)
add_subdirectory(Samples/Triangle)
add_subdirectory(Samples/Deferred)

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
