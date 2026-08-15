find_package(Threads REQUIRED)

add_library(harpia_core STATIC
    Memory/MemoryTracker.cpp
    Memory/Arena.cpp
    Threading/JobSystem.cpp
    Reflection/TypeRegistry.cpp
    Serialization/Serializer.cpp
    ECS/World.cpp)

add_library(Harpia::Core ALIAS harpia_core)

# Headers are included as <Core/...>, so Source/ is the include root.
target_include_directories(harpia_core PUBLIC ${PROJECT_SOURCE_DIR}/Source)

target_link_libraries(harpia_core
    PUBLIC  Threads::Threads
    PRIVATE harpia_warnings)

if(HARPIA_ENABLE_TRACY)
    target_link_libraries(harpia_core PUBLIC TracyClient)
    target_compile_definitions(harpia_core PUBLIC HARPIA_TRACY_ENABLED)
endif()

target_sources(harpia_core PUBLIC
    FILE_SET harpia_headers TYPE HEADERS
    BASE_DIRS ${PROJECT_SOURCE_DIR}/Source
    FILES
        Memory/MemoryTracker.h
        Memory/Arena.h
        Memory/Pool.h
        Threading/JobSystem.h
        Profiling/Profiler.h
        Reflection/TypeInfo.h
        Reflection/TypeRegistry.h
        Reflection/Reflect.h
        Serialization/ByteStream.h
        Serialization/Serializer.h
        ECS/Entity.h
        ECS/World.h)
