include(FetchContent)

FetchContent_Declare(doctest
    GIT_REPOSITORY https://github.com/doctest/doctest.git
    GIT_TAG        v2.4.12
    GIT_SHALLOW    TRUE)
FetchContent_MakeAvailable(doctest)

add_executable(harpia_tests
    main.cpp
    test_memory_tracker.cpp
    test_arena.cpp
    test_pool.cpp
    test_job_system.cpp
    test_render_golden.cpp
    test_reflection.cpp
    test_serialization.cpp)

target_include_directories(harpia_tests PRIVATE ${CMAKE_CURRENT_SOURCE_DIR})

# Harpia types carry their own toString() returning const char*, which ADL finds
# before doctest's template. This config makes doctest compose the two instead
# of failing to build, so a failing enum CHECK prints "Struct", not 12.
target_compile_definitions(harpia_tests PRIVATE DOCTEST_CONFIG_DOUBLE_STRINGIFY)

target_link_libraries(harpia_tests
    PRIVATE Harpia::Core
            Harpia::RHI
            doctest::doctest
            harpia_warnings)

# Split so leak detection stays strict where it can be. Engine code is held to
# zero leaks; the gpu suite keeps every other sanitizer check but disables leak
# detection because the Vulkan driver's JIT allocations are unattributable.
add_test(NAME harpia_engine
         COMMAND harpia_tests --test-suite-exclude=gpu)
add_test(NAME harpia_gpu
         COMMAND harpia_tests --test-suite=gpu)

set_tests_properties(harpia_engine PROPERTIES TIMEOUT 300)
set_tests_properties(harpia_gpu PROPERTIES
    TIMEOUT 300
    ENVIRONMENT "ASAN_OPTIONS=detect_leaks=0;LSAN_OPTIONS=suppressions=${CMAKE_CURRENT_SOURCE_DIR}/lsan.supp")
