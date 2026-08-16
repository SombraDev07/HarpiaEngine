# Harpia Engine — where every third-party dependency is pinned
#
# One file so the whole dependency surface is readable at once. Nothing is
# vendored: a `ThirdParty/` tree of copied sources is a large part of what makes
# an engine impenetrable from the outside, and it buys nothing here — every
# library below has a real repository and a stable tag. Vendoring earns its
# place only for a library we have to patch, or one with no usable build of its
# own; when that day comes, NOTICE.md stops being optional.
#
# Tags are exact on purpose. A floating branch turns "works on my machine" into
# a property of the day someone cloned.
#
# The shader compiler is deliberately not here. glslang is a build-tool
# fallback, fetched only when the system has no dxc, and it lives next to the
# detection that decides whether it is needed at all — see HarpiaShaders.cmake.

include(FetchContent)
set(FETCHCONTENT_QUIET OFF)

# --- system packages ----------------------------------------------------------
find_package(Vulkan REQUIRED)
find_package(glfw3 3.3 REQUIRED)

# --- vma ----------------------------------------------------------------------
FetchContent_Declare(vma
    GIT_REPOSITORY https://github.com/GPUOpen-LibrariesAndSDKs/VulkanMemoryAllocator.git
    GIT_TAG        v3.4.0
    GIT_SHALLOW    TRUE)
FetchContent_MakeAvailable(vma)

# --- stb ----------------------------------------------------------------------
# stb ships headers only, with no build system of its own.
FetchContent_Declare(stb
    GIT_REPOSITORY https://github.com/nothings/stb.git
    GIT_TAG        f0569113c93ad095470c54bf34a17b36646bbbb5
    GIT_SHALLOW    FALSE)
FetchContent_MakeAvailable(stb)

add_library(stb INTERFACE)
target_include_directories(stb SYSTEM INTERFACE ${stb_SOURCE_DIR})

# --- glm ----------------------------------------------------------------------
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

# --- cgltf --------------------------------------------------------------------
FetchContent_Declare(cgltf
    GIT_REPOSITORY https://github.com/jkuhlmann/cgltf.git
    GIT_TAG        v1.15
    GIT_SHALLOW    TRUE)
FetchContent_MakeAvailable(cgltf)

add_library(cgltf INTERFACE)
target_include_directories(cgltf SYSTEM INTERFACE ${cgltf_SOURCE_DIR})

# --- tracy --------------------------------------------------------------------
if(HARPIA_ENABLE_TRACY)
    FetchContent_Declare(tracy
        GIT_REPOSITORY https://github.com/wolfpld/tracy.git
        GIT_TAG        v0.11.1
        GIT_SHALLOW    TRUE)
    set(TRACY_ENABLE ON  CACHE BOOL "" FORCE)
    set(TRACY_ON_DEMAND ON CACHE BOOL "" FORCE)
    FetchContent_MakeAvailable(tracy)
endif()

# --- jolt ---------------------------------------------------------------------
# Physics 4.3: Jolt is the solver. We do not wrap it behind a backend
# interface — that only pays when a second backend exists. GPU compute in
# 5.6+ is unused here and would pull Vulkan into a layer that must not see it.
FetchContent_Declare(JoltPhysics
    GIT_REPOSITORY https://github.com/jrouwe/JoltPhysics.git
    GIT_TAG        v5.6.0
    GIT_SHALLOW    TRUE
    SOURCE_SUBDIR  Build)

set(ENABLE_ALL_WARNINGS OFF CACHE BOOL "" FORCE)
set(OVERRIDE_CXX_FLAGS OFF CACHE BOOL "" FORCE)
set(INTERPROCEDURAL_OPTIMIZATION OFF CACHE BOOL "" FORCE)
set(ENABLE_INSTALL OFF CACHE BOOL "" FORCE)
set(ENABLE_OBJECT_STREAM OFF CACHE BOOL "" FORCE)
set(DEBUG_RENDERER_IN_DEBUG_AND_RELEASE OFF CACHE BOOL "" FORCE)
set(PROFILER_IN_DEBUG_AND_RELEASE OFF CACHE BOOL "" FORCE)
set(FLOATING_POINT_EXCEPTIONS_ENABLED OFF CACHE BOOL "" FORCE)
set(CPP_RTTI_ENABLED ON CACHE BOOL "" FORCE)
set(JPH_USE_DX12 OFF CACHE BOOL "" FORCE)
set(JPH_USE_VK OFF CACHE BOOL "" FORCE)
set(JPH_USE_MTL OFF CACHE BOOL "" FORCE)
set(JPH_USE_CPU_COMPUTE OFF CACHE BOOL "" FORCE)
FetchContent_MakeAvailable(JoltPhysics)

# Jolt headers trip -Wconversion / -Wsign-conversion; they are not ours to fix.
# A SYSTEM wrapper is the same pattern as glm, stb and cgltf.
add_library(harpia_jolt INTERFACE)
target_link_libraries(harpia_jolt INTERFACE Jolt)
FetchContent_GetProperties(JoltPhysics SOURCE_DIR JoltPhysics_SOURCE_DIR)
if(JoltPhysics_SOURCE_DIR)
    target_include_directories(harpia_jolt SYSTEM INTERFACE ${JoltPhysics_SOURCE_DIR})
endif()

# --- doctest ------------------------------------------------------------------
# Fetched here rather than in Tests/ so every pinned tag is visible in one
# place, but still only when tests are actually being built.
if(HARPIA_BUILD_TESTS)
    FetchContent_Declare(doctest
        GIT_REPOSITORY https://github.com/doctest/doctest.git
        GIT_TAG        v2.4.12
        GIT_SHALLOW    TRUE)
    FetchContent_MakeAvailable(doctest)
endif()
