# Harpia Engine — HLSL to SPIR-V
#
# Roadmap decision: HLSL compiled by DXC. DXC is preferred and looked for first;
# glslang's HLSL front end is the fallback so a clean checkout builds with no
# manual setup. When DXC appears on PATH the build upgrades to it.
#
# Deliberately not a shader language of our own. DSHL is the single heaviest
# piece of dead weight in Dagor: 1010 files tied to a compiler they maintain
# forever.
#
# Shaders are compiled once into a single `harpia_shaders` target. Two targets
# asking for the same shader must not each declare a rule producing the same
# file — ninja rejects duplicate outputs, and one of them silently winning is
# worse.

set(HARPIA_SHADER_OUTPUT_DIR "${CMAKE_BINARY_DIR}/shaders")

# A macro, not a function: the compiler choice has to land in the including
# scope so every later add_subdirectory can see it.
macro(_harpia_find_shader_compiler)
    find_program(HARPIA_DXC NAMES dxc HINTS ENV VULKAN_SDK PATH_SUFFIXES bin)
    find_program(HARPIA_GLSLANG NAMES glslang glslangValidator
                 HINTS ENV VULKAN_SDK PATH_SUFFIXES bin)

    set(HARPIA_SHADER_COMPILER_TARGET "")

    if(HARPIA_DXC)
        set(HARPIA_SHADER_COMPILER "${HARPIA_DXC}")
        set(HARPIA_SHADER_COMPILER_KIND "dxc")
        message(STATUS "Shader compiler: dxc (${HARPIA_DXC})")
    elseif(HARPIA_GLSLANG)
        set(HARPIA_SHADER_COMPILER "${HARPIA_GLSLANG}")
        set(HARPIA_SHADER_COMPILER_KIND "glslang")
        message(STATUS "Shader compiler: glslang (${HARPIA_GLSLANG}) — install dxc for SM6.x")
    else()
        message(STATUS "Shader compiler: none found, building glslang from source")

        set(GLSLANG_TESTS OFF CACHE BOOL "" FORCE)
        set(GLSLANG_ENABLE_INSTALL OFF CACHE BOOL "" FORCE)
        set(ENABLE_OPT OFF CACHE BOOL "" FORCE)
        set(ENABLE_HLSL ON CACHE BOOL "" FORCE)
        set(BUILD_TESTING OFF CACHE BOOL "" FORCE)

        include(FetchContent)
        FetchContent_Declare(glslang
            GIT_REPOSITORY https://github.com/KhronosGroup/glslang.git
            GIT_TAG        15.4.0
            GIT_SHALLOW    TRUE)
        FetchContent_MakeAvailable(glslang)

        # Not cached: the generator expression is only valid once the target
        # exists, and a cached copy would outlive the target on reconfigure.
        set(HARPIA_SHADER_COMPILER "$<TARGET_FILE:glslang-standalone>")
        set(HARPIA_SHADER_COMPILER_KIND "glslang")
        set(HARPIA_SHADER_COMPILER_TARGET "glslang-standalone")
    endif()
endmacro()

_harpia_find_shader_compiler()

find_program(HARPIA_SPIRV_VAL NAMES spirv-val HINTS ENV VULKAN_SDK PATH_SUFFIXES bin)
if(HARPIA_SPIRV_VAL)
    message(STATUS "SPIR-V validator: ${HARPIA_SPIRV_VAL}")
else()
    message(STATUS "SPIR-V validator: not found — shaders will not be validated")
endif()

# harpia_declare_shaders(SOURCES <files...>)
#
# Call once. Stage comes from the filename: Foo.vert.hlsl -> vert.
function(harpia_declare_shaders)
    cmake_parse_arguments(ARG "" "" "SOURCES" ${ARGN})

    # Every shader depends on every .hlsli: editing a shared struct must rebuild
    # everything that mirrors it, and tracking real include graphs is not worth
    # the machinery at this size.
    file(GLOB HARPIA_SHADER_HEADERS "${CMAKE_CURRENT_FUNCTION_LIST_DIR}/../Shaders/*.hlsli")

    file(MAKE_DIRECTORY "${HARPIA_SHADER_OUTPUT_DIR}")
    set(outputs "")

    foreach(source IN LISTS ARG_SOURCES)
        get_filename_component(absolute "${source}" ABSOLUTE)
        get_filename_component(filename "${source}" NAME)

        # Foo.vert.hlsl -> stem "Foo.vert", stage "vert"
        string(REGEX REPLACE "\\.hlsl$" "" stem "${filename}")
        if(NOT stem MATCHES "\\.(vert|frag|comp)$")
            message(FATAL_ERROR
                "Shader ${filename} must be named <name>.<vert|frag|comp>.hlsl")
        endif()
        string(REGEX REPLACE "^.*\\.([a-z]+)$" "\\1" stage "${stem}")

        set(output "${HARPIA_SHADER_OUTPUT_DIR}/${stem}.spv")

        if(HARPIA_SHADER_COMPILER_KIND STREQUAL "dxc")
            if(stage STREQUAL "vert")
                set(profile "vs_6_0")
            elseif(stage STREQUAL "frag")
                set(profile "ps_6_0")
            else()
                set(profile "cs_6_0")
            endif()
            # Scalar layout makes a struct in a buffer pack exactly like the C++
            # mirror. Without it HLSL aligns float3 to 16 bytes, so a Vertex
            # would stride 64 on the GPU against 48 on the CPU and every read
            # would land on the wrong field. The device enables
            # scalarBlockLayout for precisely this.
            set(command ${HARPIA_SHADER_COMPILER}
                -spirv -fspv-target-env=vulkan1.3 -fvk-use-scalar-layout
                -T ${profile} -E main
                -I "${CMAKE_CURRENT_FUNCTION_LIST_DIR}/../Shaders"
                -Fo "${output}" "${absolute}")
        else()
            # -D tells glslang the input is HLSL rather than GLSL.
            set(command ${HARPIA_SHADER_COMPILER}
                -D -e main --target-env vulkan1.3
                -S ${stage} -V
                -I"${CMAKE_CURRENT_FUNCTION_LIST_DIR}/../Shaders"
                -o "${output}" "${absolute}")
        endif()

        # Validating here turns a malformed module into a build failure instead
        # of a driver crash or, worse, silently wrong pixels.
        set(validate_command "")
        if(HARPIA_SPIRV_VAL)
            set(validate_command
                COMMAND ${HARPIA_SPIRV_VAL} --target-env vulkan1.3
                        --scalar-block-layout "${output}")
        endif()

        add_custom_command(
            OUTPUT "${output}"
            COMMAND ${command}
            ${validate_command}
            DEPENDS "${absolute}" ${HARPIA_SHADER_HEADERS} ${HARPIA_SHADER_COMPILER_TARGET}
            COMMENT "Compiling shader ${stem}"
            VERBATIM)

        list(APPEND outputs "${output}")
    endforeach()

    add_custom_target(harpia_shaders ALL DEPENDS ${outputs})
endfunction()

# Attaches the shader build to a target and tells its code where to find the
# results. A stale shader can then never survive a build of that target.
function(harpia_use_shaders TARGET)
    add_dependencies(${TARGET} harpia_shaders)
    target_compile_definitions(${TARGET} PRIVATE
        HARPIA_SHADER_DIR="${HARPIA_SHADER_OUTPUT_DIR}")
endfunction()
