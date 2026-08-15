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
    test_render_golden.cpp)

target_include_directories(harpia_tests PRIVATE ${CMAKE_CURRENT_SOURCE_DIR})

target_link_libraries(harpia_tests
    PRIVATE Harpia::Core
            Harpia::RHI
            doctest::doctest
            harpia_warnings)

add_test(NAME harpia_tests COMMAND harpia_tests)

# A hung job system should fail the build, not the build machine.
set_tests_properties(harpia_tests PROPERTIES TIMEOUT 120)
