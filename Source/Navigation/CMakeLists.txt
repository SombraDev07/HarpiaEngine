add_library(harpia_navigation STATIC
    RecastAllocator.cpp
    NavMesh.cpp
    Crowd.cpp
    BehaviorTree.cpp
    NavigationComponents.cpp
    NavigationWorld.cpp)

add_library(Harpia::Navigation ALIAS harpia_navigation)

target_include_directories(harpia_navigation PUBLIC ${PROJECT_SOURCE_DIR}/Source)

target_link_libraries(harpia_navigation
    PUBLIC  Harpia::Core
            harpia_recast
    PRIVATE harpia_warnings)

target_sources(harpia_navigation PUBLIC
    FILE_SET harpia_navigation_headers TYPE HEADERS
    BASE_DIRS ${PROJECT_SOURCE_DIR}/Source
    FILES
        NavMesh.h
        Crowd.h
        BehaviorTree.h
        PerceptionLod.h
        NavigationComponents.h
        NavigationWorld.h)
