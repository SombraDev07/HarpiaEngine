find_package(Threads REQUIRED)

add_library(harpia_core STATIC
    Math/Math.cpp
    Memory/MemoryTracker.cpp
    Memory/Arena.cpp
    Threading/JobSystem.cpp
    Reflection/TypeRegistry.cpp
    Serialization/Serializer.cpp
    ECS/World.cpp
    Assets/AssetId.cpp
    Assets/AssetDatabase.cpp
    Assets/AssetManager.cpp
    Assets/GltfLoader.cpp
    Assets/CgltfImpl.cpp)

add_library(Harpia::Core ALIAS harpia_core)

# Headers are included as <Core/...>, so Source/ is the include root.
target_include_directories(harpia_core PUBLIC ${PROJECT_SOURCE_DIR}/Source)

target_link_libraries(harpia_core
    PUBLIC  Threads::Threads
            cgltf
            harpia_glm
    PRIVATE harpia_warnings)

if(HARPIA_ENABLE_TRACY)
    target_link_libraries(harpia_core PUBLIC TracyClient)
    target_compile_definitions(harpia_core PUBLIC HARPIA_TRACY_ENABLED)
endif()

target_sources(harpia_core PUBLIC
    FILE_SET harpia_headers TYPE HEADERS
    BASE_DIRS ${PROJECT_SOURCE_DIR}/Source
    FILES
        Math/Math.h
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
        ECS/World.h
        Assets/AssetId.h
        Assets/AssetDatabase.h
        Assets/AssetManager.h
        Assets/MeshAsset.h
        Assets/GltfLoader.h)

# cgltf is a single-header library; its warnings are not ours to fix.
set_source_files_properties(Assets/CgltfImpl.cpp PROPERTIES COMPILE_OPTIONS "-w")
