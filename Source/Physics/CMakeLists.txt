add_library(harpia_physics STATIC
    JoltAllocator.cpp
    JoltJobSystem.cpp
    PhysicsComponents.cpp
    PhysicsWorld.cpp)

add_library(Harpia::Physics ALIAS harpia_physics)

target_include_directories(harpia_physics PUBLIC ${PROJECT_SOURCE_DIR}/Source)

target_link_libraries(harpia_physics
    PUBLIC  Harpia::Core
            harpia_jolt
    PRIVATE harpia_warnings)

target_sources(harpia_physics PUBLIC
    FILE_SET harpia_physics_headers TYPE HEADERS
    BASE_DIRS ${PROJECT_SOURCE_DIR}/Source
    FILES
        PhysicsTypes.h
        PhysicsComponents.h
        PhysicsWorld.h)
