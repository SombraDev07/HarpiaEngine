// Harpia Engine — archetype world
//
// Entities with the same component set share an archetype; each archetype
// stores its entities in 16 KB chunks, one tightly packed array per component.
// That is what makes a query a linear walk over exactly the components it asked
// for, with no per-entity branching and no pointer chasing.
//
// Adding or removing a component moves the entity to a different archetype.
// That move is the cost of the layout, and it is why component churn belongs
// outside the inner loop.
#pragma once

#include "Core/ECS/Entity.h"
#include "Core/Memory/MemoryTracker.h"

#include <cstddef>
#include <cstdint>
#include <functional>
#include <memory>
#include <vector>

namespace harpia::ecs {

class World {
public:
    // 16 KB keeps a chunk inside L2 on every target we care about while still
    // holding enough entities that per-chunk overhead disappears.
    static constexpr std::size_t kChunkBytes = 16 * 1024;

    World();
    ~World();

    World(const World&)            = delete;
    World& operator=(const World&) = delete;

    // --- entity lifetime ---------------------------------------------------

    [[nodiscard]] Entity create();

    template <typename... Cs>
    [[nodiscard]] Entity create(Cs&&... components)
    {
        const Entity entity = createWithMask(maskOf<std::decay_t<Cs>...>());
        (assign<std::decay_t<Cs>>(entity, std::forward<Cs>(components)), ...);
        return entity;
    }

    void destroy(Entity entity);
    [[nodiscard]] bool alive(Entity entity) const noexcept;
    [[nodiscard]] std::size_t size() const noexcept { return liveCount_; }
    void clear();

    // --- components --------------------------------------------------------

    template <typename T>
    void add(Entity entity, T value = T{})
    {
        addRaw(entity, ComponentRegistry::id<T>(), &value);
    }

    template <typename T>
    void remove(Entity entity)
    {
        removeRaw(entity, ComponentRegistry::id<T>());
    }

    template <typename T>
    [[nodiscard]] bool has(Entity entity) const
    {
        return hasRaw(entity, ComponentRegistry::id<T>());
    }

    // Returns nullptr when the entity is dead or lacks the component. The
    // pointer is invalidated by any structural change to the same archetype.
    template <typename T>
    [[nodiscard]] T* get(Entity entity)
    {
        return static_cast<T*>(getRaw(entity, ComponentRegistry::id<T>()));
    }

    template <typename T>
    [[nodiscard]] const T* get(Entity entity) const
    {
        return static_cast<const T*>(
            const_cast<World*>(this)->getRaw(entity, ComponentRegistry::id<T>()));
    }

    // --- queries -----------------------------------------------------------

    // fn is called as fn(Entity, Cs&...) once per matching entity.
    template <typename... Cs, typename Fn>
    void each(Fn&& fn)
    {
        const ComponentMask required = maskOf<Cs...>();
        forEachChunk(required, [&](const ChunkView& view) {
            invokeChunk<Cs...>(view, fn);
        });
    }

    // Same, one job per chunk. Chunks are disjoint, so a function that only
    // touches its own entities needs no synchronisation of its own.
    template <typename... Cs, typename Fn>
    void parallelEach(Fn&& fn)
    {
        const ComponentMask required = maskOf<Cs...>();
        std::vector<ChunkView> views;
        forEachChunk(required, [&](const ChunkView& view) { views.push_back(view); });
        dispatchParallel(views, [&](const ChunkView& view) {
            invokeChunk<Cs...>(view, fn);
        });
    }

    [[nodiscard]] std::size_t archetypeCount() const noexcept;
    [[nodiscard]] std::size_t chunkCount() const noexcept;

private:
    struct Chunk;
    struct Archetype;

    struct ChunkView {
        const Archetype* archetype = nullptr;
        Chunk*           chunk     = nullptr;
        Entity*          entities  = nullptr;
        std::uint32_t    count     = 0;
    };

    // Indices come from an index_sequence rather than a running counter: the
    // evaluation order of a pack expansion inside a call is unspecified, so a
    // cursor would silently pair the wrong array with the wrong component.
    template <typename... Cs, typename Fn, std::size_t... Is>
    void invokeChunkImpl(const ChunkView& view, Fn& fn, std::index_sequence<Is...>)
    {
        // One base pointer per requested component, resolved once per chunk.
        void* bases[sizeof...(Cs)] = {componentArray(view, ComponentRegistry::id<Cs>())...};

        for (std::uint32_t row = 0; row < view.count; ++row) {
            fn(view.entities[row], static_cast<Cs*>(bases[Is])[row]...);
        }
    }

    template <typename... Cs, typename Fn>
    void invokeChunk(const ChunkView& view, Fn& fn)
    {
        if constexpr (sizeof...(Cs) == 0) {
            for (std::uint32_t row = 0; row < view.count; ++row) {
                fn(view.entities[row]);
            }
        } else {
            invokeChunkImpl<Cs...>(view, fn, std::index_sequence_for<Cs...>{});
        }
    }

    [[nodiscard]] Entity createWithMask(const ComponentMask& mask);

    template <typename T>
    void assign(Entity entity, T&& value)
    {
        using Bare = std::decay_t<T>;
        if (auto* slot = get<Bare>(entity)) {
            *slot = std::forward<T>(value);
        }
    }

    void  addRaw(Entity entity, ComponentId component, const void* value);
    void  removeRaw(Entity entity, ComponentId component);
    [[nodiscard]] bool  hasRaw(Entity entity, ComponentId component) const noexcept;
    [[nodiscard]] void* getRaw(Entity entity, ComponentId component);

    void forEachChunk(const ComponentMask&                     required,
                      const std::function<void(const ChunkView&)>& body);

    void dispatchParallel(const std::vector<ChunkView>&                views,
                          const std::function<void(const ChunkView&)>& body);

    [[nodiscard]] static void* componentArray(const ChunkView& view,
                                              ComponentId      component);

    struct Impl;
    std::unique_ptr<Impl> impl_;
    std::size_t           liveCount_ = 0;
};

} // namespace harpia::ecs
