#include "Core/ECS/World.h"

#include "Core/Threading/JobSystem.h"

#include <algorithm>
#include <cassert>
#include <mutex>
#include <unordered_map>
#include <vector>

namespace harpia::ecs {
namespace {

constexpr std::uint32_t kInvalidArchetype = 0xFFFFFFFFu;

[[nodiscard]] constexpr std::size_t alignUp(std::size_t value, std::size_t alignment) noexcept
{
    return (value + alignment - 1) & ~(alignment - 1);
}

struct ComponentSlot {
    const reflect::TypeInfo* type = nullptr;
};

struct ComponentTable {
    std::mutex                                  mutex;
    std::vector<ComponentSlot>                  slots;
    std::unordered_map<std::string, ComponentId> byName;
};

ComponentTable& componentTable()
{
    static ComponentTable table;
    return table;
}

} // namespace

// --- ComponentRegistry ------------------------------------------------------

ComponentId ComponentRegistry::add(const reflect::TypeInfo* type)
{
    ComponentTable& table = componentTable();
    std::lock_guard<std::mutex> lock(table.mutex);

    const auto existing = table.byName.find(type->name);
    if (existing != table.byName.end()) {
        return existing->second;
    }

    assert(table.slots.size() < kMaxComponentTypes
           && "component type budget exhausted; raise kMaxComponentTypes");

    const auto id = static_cast<ComponentId>(table.slots.size());
    table.slots.push_back(ComponentSlot{type});
    table.byName.emplace(type->name, id);
    return id;
}

const reflect::TypeInfo* ComponentRegistry::typeInfo(ComponentId id) noexcept
{
    ComponentTable& table = componentTable();
    std::lock_guard<std::mutex> lock(table.mutex);
    return id < table.slots.size() ? table.slots[id].type : nullptr;
}

ComponentId ComponentRegistry::findByName(std::string_view name) noexcept
{
    ComponentTable& table = componentTable();
    std::lock_guard<std::mutex> lock(table.mutex);
    const auto it = table.byName.find(std::string{name});
    return it != table.byName.end() ? it->second : kInvalidComponent;
}

std::size_t ComponentRegistry::count() noexcept
{
    ComponentTable& table = componentTable();
    std::lock_guard<std::mutex> lock(table.mutex);
    return table.slots.size();
}

// --- internals --------------------------------------------------------------

struct World::Chunk {
    std::unique_ptr<std::byte[]> data;
    std::uint32_t                count = 0;
};

struct World::Archetype {
    ComponentMask            mask;
    std::vector<ComponentId> components;   // ascending, so layout is deterministic
    std::vector<std::size_t> offsets;      // byte offset of each component array
    std::size_t              entityOffset = 0;
    std::uint32_t            capacity     = 0;
    std::vector<Chunk>       chunks;
};

struct World::Impl {
    std::vector<Archetype> archetypes;

    // Mask -> archetype index. Rebuilt never; archetypes are permanent once
    // created, so queries can cache against them later.
    std::vector<std::pair<ComponentMask, std::uint32_t>> lookup;

    struct Record {
        std::uint32_t archetype  = kInvalidArchetype;
        std::uint32_t chunk      = 0;
        std::uint32_t row        = 0;
        std::uint32_t generation = 0; // odd = alive
    };

    std::vector<Record>        records;
    std::vector<std::uint32_t> freeIndices;

    [[nodiscard]] std::uint32_t findOrCreateArchetype(const ComponentMask& mask);
    [[nodiscard]] bool          isLive(Entity entity) const noexcept;

    void   allocateRow(std::uint32_t archetypeIndex, Entity entity,
                       std::uint32_t& outChunk, std::uint32_t& outRow);
    void   releaseRow(std::uint32_t archetypeIndex, std::uint32_t chunkIndex,
                      std::uint32_t row);
    void   moveEntity(Entity entity, std::uint32_t targetArchetype);

    [[nodiscard]] static void* arrayAt(const Archetype& archetype, Chunk& chunk,
                                       std::size_t componentSlot)
    {
        return chunk.data.get() + archetype.offsets[componentSlot];
    }

    [[nodiscard]] static Entity* entityArray(const Archetype& archetype, Chunk& chunk)
    {
        return reinterpret_cast<Entity*>(chunk.data.get() + archetype.entityOffset);
    }

    [[nodiscard]] static std::size_t slotOf(const Archetype& archetype, ComponentId component)
    {
        const auto it = std::lower_bound(archetype.components.begin(),
                                         archetype.components.end(), component);
        if (it == archetype.components.end() || *it != component) {
            return static_cast<std::size_t>(-1);
        }
        return static_cast<std::size_t>(it - archetype.components.begin());
    }
};

std::uint32_t World::Impl::findOrCreateArchetype(const ComponentMask& mask)
{
    for (const auto& [candidateMask, index] : lookup) {
        if (candidateMask == mask) {
            return index;
        }
    }

    Archetype archetype;
    archetype.mask = mask;
    for (std::size_t i = 0; i < kMaxComponentTypes; ++i) {
        if (mask.test(i)) {
            archetype.components.push_back(static_cast<ComponentId>(i));
        }
    }

    // Solve for the entity count that fits in one chunk: the entity array plus
    // one aligned array per component.
    std::size_t perEntity = sizeof(Entity);
    std::size_t alignSlack = alignof(Entity);
    for (const ComponentId component : archetype.components) {
        const reflect::TypeInfo* type = ComponentRegistry::typeInfo(component);
        perEntity += type->size;
        alignSlack += type->alignment;
    }

    std::uint32_t capacity = perEntity > 0
        ? static_cast<std::uint32_t>((kChunkBytes - alignSlack) / perEntity)
        : 1;
    if (capacity == 0) {
        capacity = 1;
    }

    // Lay the arrays out and shrink until they genuinely fit.
    for (;;) {
        std::size_t cursor = 0;
        archetype.entityOffset = cursor;
        cursor += sizeof(Entity) * capacity;

        archetype.offsets.clear();
        for (const ComponentId component : archetype.components) {
            const reflect::TypeInfo* type = ComponentRegistry::typeInfo(component);
            cursor = alignUp(cursor, type->alignment);
            archetype.offsets.push_back(cursor);
            cursor += type->size * capacity;
        }

        if (cursor <= kChunkBytes || capacity == 1) {
            break;
        }
        capacity = capacity > 8 ? capacity - (capacity / 8) : capacity - 1;
    }

    archetype.capacity = capacity;

    const auto index = static_cast<std::uint32_t>(archetypes.size());
    archetypes.push_back(std::move(archetype));
    lookup.emplace_back(mask, index);
    return index;
}

bool World::Impl::isLive(Entity entity) const noexcept
{
    return entity.valid()
        && entity.index < records.size()
        && records[entity.index].generation == entity.generation;
}

void World::Impl::allocateRow(std::uint32_t archetypeIndex, Entity entity,
                              std::uint32_t& outChunk, std::uint32_t& outRow)
{
    Archetype& archetype = archetypes[archetypeIndex];

    std::uint32_t chunkIndex = kInvalidArchetype;
    for (std::uint32_t i = 0; i < archetype.chunks.size(); ++i) {
        if (archetype.chunks[i].count < archetype.capacity) {
            chunkIndex = i;
            break;
        }
    }

    if (chunkIndex == kInvalidArchetype) {
        Chunk chunk;
        chunk.data = std::unique_ptr<std::byte[]>(new std::byte[kChunkBytes]);
        MemoryTracker::recordAlloc(MemTag::Scene, kChunkBytes);
        archetype.chunks.push_back(std::move(chunk));
        chunkIndex = static_cast<std::uint32_t>(archetype.chunks.size() - 1);
    }

    Chunk& chunk = archetype.chunks[chunkIndex];
    const std::uint32_t row = chunk.count++;

    entityArray(archetype, chunk)[row] = entity;

    // Components start default-constructed; add() overwrites immediately.
    for (std::size_t slot = 0; slot < archetype.components.size(); ++slot) {
        const reflect::TypeInfo* type = ComponentRegistry::typeInfo(archetype.components[slot]);
        auto* base = static_cast<std::byte*>(arrayAt(archetype, chunk, slot));
        if (type->construct != nullptr) {
            type->construct(base + type->size * row);
        }
    }

    outChunk = chunkIndex;
    outRow   = row;
}

void World::Impl::releaseRow(std::uint32_t archetypeIndex, std::uint32_t chunkIndex,
                             std::uint32_t row)
{
    Archetype& archetype = archetypes[archetypeIndex];
    Chunk&     chunk     = archetype.chunks[chunkIndex];

    const std::uint32_t last = chunk.count - 1;

    for (std::size_t slot = 0; slot < archetype.components.size(); ++slot) {
        const reflect::TypeInfo* type = ComponentRegistry::typeInfo(archetype.components[slot]);
        auto* base = static_cast<std::byte*>(arrayAt(archetype, chunk, slot));

        std::byte* target = base + type->size * row;
        type->destruct(target);

        // Swap-remove: the last row moves into the hole so the array stays
        // packed and iteration never has to skip anything.
        if (row != last) {
            std::byte* source = base + type->size * last;
            if (type->moveConstruct != nullptr) {
                type->moveConstruct(target, source);
            } else if (type->copyConstruct != nullptr) {
                type->copyConstruct(target, source);
            }
            type->destruct(source);
        }
    }

    Entity* entities = entityArray(archetype, chunk);
    if (row != last) {
        entities[row] = entities[last];
        records[entities[row].index].row = row;
    }
    --chunk.count;
}

void World::Impl::moveEntity(Entity entity, std::uint32_t targetArchetype)
{
    Record& record = records[entity.index];

    const std::uint32_t sourceArchetype = record.archetype;
    const std::uint32_t sourceChunk     = record.chunk;
    const std::uint32_t sourceRow       = record.row;

    std::uint32_t newChunk = 0;
    std::uint32_t newRow   = 0;
    allocateRow(targetArchetype, entity, newChunk, newRow);

    // Relocate every component the two archetypes share.
    if (sourceArchetype != kInvalidArchetype) {
        Archetype& from = archetypes[sourceArchetype];
        Archetype& to   = archetypes[targetArchetype];
        Chunk&     fromChunk = from.chunks[sourceChunk];
        Chunk&     toChunk   = to.chunks[newChunk];

        for (std::size_t slot = 0; slot < from.components.size(); ++slot) {
            const ComponentId component = from.components[slot];
            const std::size_t targetSlot = slotOf(to, component);
            if (targetSlot == static_cast<std::size_t>(-1)) {
                continue; // component is being removed
            }

            const reflect::TypeInfo* type = ComponentRegistry::typeInfo(component);
            auto* source = static_cast<std::byte*>(arrayAt(from, fromChunk, slot))
                         + type->size * sourceRow;
            auto* target = static_cast<std::byte*>(arrayAt(to, toChunk, targetSlot))
                         + type->size * newRow;

            type->destruct(target); // undo the default construction
            if (type->moveConstruct != nullptr) {
                type->moveConstruct(target, source);
            } else if (type->copyConstruct != nullptr) {
                type->copyConstruct(target, source);
            }
        }

        releaseRow(sourceArchetype, sourceChunk, sourceRow);
    }

    record.archetype = targetArchetype;
    record.chunk     = newChunk;
    record.row       = newRow;
}

// --- World ------------------------------------------------------------------

World::World() : impl_(std::make_unique<Impl>()) {}

World::~World()
{
    clear();
    for (Archetype& archetype : impl_->archetypes) {
        MemoryTracker::recordFree(MemTag::Scene, kChunkBytes * archetype.chunks.size());
    }
}

Entity World::create()
{
    return createWithMask(ComponentMask{});
}

Entity World::createWithMask(const ComponentMask& mask)
{
    std::uint32_t index = 0;
    if (!impl_->freeIndices.empty()) {
        index = impl_->freeIndices.back();
        impl_->freeIndices.pop_back();
    } else {
        index = static_cast<std::uint32_t>(impl_->records.size());
        impl_->records.emplace_back();
    }

    Impl::Record& record = impl_->records[index];
    record.generation += 1; // even -> odd: alive

    const Entity entity{index, record.generation};

    const std::uint32_t archetype = impl_->findOrCreateArchetype(mask);
    record.archetype = kInvalidArchetype;
    impl_->moveEntity(entity, archetype);

    ++liveCount_;
    return entity;
}

void World::destroy(Entity entity)
{
    if (!impl_->isLive(entity)) {
        return;
    }

    Impl::Record& record = impl_->records[entity.index];
    impl_->releaseRow(record.archetype, record.chunk, record.row);

    record.generation += 1; // odd -> even: handle goes stale
    record.archetype = kInvalidArchetype;
    impl_->freeIndices.push_back(entity.index);
    --liveCount_;
}

bool World::alive(Entity entity) const noexcept
{
    return impl_->isLive(entity);
}

void World::clear()
{
    for (std::uint32_t index = 0; index < impl_->records.size(); ++index) {
        Impl::Record& record = impl_->records[index];
        if ((record.generation & 1u) != 0u) {
            destroy(Entity{index, record.generation});
        }
    }
}

void World::addRaw(Entity entity, ComponentId component, const void* value)
{
    if (!impl_->isLive(entity)) {
        return;
    }

    Impl::Record& record = impl_->records[entity.index];
    ComponentMask mask   = impl_->archetypes[record.archetype].mask;

    if (!mask.test(component)) {
        mask.set(component);
        impl_->moveEntity(entity, impl_->findOrCreateArchetype(mask));
    }

    if (value != nullptr) {
        void* slot = getRaw(entity, component);
        if (slot != nullptr) {
            const reflect::TypeInfo* type = ComponentRegistry::typeInfo(component);
            type->destruct(slot);
            if (type->copyConstruct != nullptr) {
                type->copyConstruct(slot, value);
            }
        }
    }
}

void World::removeRaw(Entity entity, ComponentId component)
{
    if (!impl_->isLive(entity)) {
        return;
    }

    Impl::Record& record = impl_->records[entity.index];
    ComponentMask mask   = impl_->archetypes[record.archetype].mask;
    if (!mask.test(component)) {
        return;
    }

    mask.reset(component);
    impl_->moveEntity(entity, impl_->findOrCreateArchetype(mask));
}

bool World::hasRaw(Entity entity, ComponentId component) const noexcept
{
    if (!impl_->isLive(entity)) {
        return false;
    }
    const Impl::Record& record = impl_->records[entity.index];
    return impl_->archetypes[record.archetype].mask.test(component);
}

void* World::getRaw(Entity entity, ComponentId component)
{
    if (!impl_->isLive(entity)) {
        return nullptr;
    }

    const Impl::Record& record    = impl_->records[entity.index];
    Archetype&          archetype = impl_->archetypes[record.archetype];

    const std::size_t slot = Impl::slotOf(archetype, component);
    if (slot == static_cast<std::size_t>(-1)) {
        return nullptr;
    }

    Chunk& chunk = archetype.chunks[record.chunk];
    const reflect::TypeInfo* type = ComponentRegistry::typeInfo(component);
    return static_cast<std::byte*>(Impl::arrayAt(archetype, chunk, slot))
         + type->size * record.row;
}

void World::forEachChunk(const ComponentMask&                        required,
                         const std::function<void(const ChunkView&)>& body)
{
    for (Archetype& archetype : impl_->archetypes) {
        // Superset test: the archetype must have at least what was asked for.
        if ((archetype.mask & required) != required) {
            continue;
        }
        for (Chunk& chunk : archetype.chunks) {
            if (chunk.count == 0) {
                continue;
            }
            ChunkView view;
            view.archetype = &archetype;
            view.chunk     = &chunk;
            view.entities  = Impl::entityArray(archetype, chunk);
            view.count     = chunk.count;
            body(view);
        }
    }
}

void World::dispatchParallel(const std::vector<ChunkView>&                views,
                             const std::function<void(const ChunkView&)>& body)
{
    if (views.empty()) {
        return;
    }

    JobSystem& jobs = JobSystem::get();
    if (!jobs.initialized() || views.size() == 1) {
        for (const ChunkView& view : views) {
            body(view);
        }
        return;
    }

    // One job per chunk. Chunks are disjoint, so a body that only touches its
    // own entities needs no locking.
    jobs.parallelFor(views.size(), 1, [&](std::size_t begin, std::size_t end) {
        for (std::size_t i = begin; i < end; ++i) {
            body(views[i]);
        }
    }, "ecs_query");
}

void* World::componentArray(const ChunkView& view, ComponentId component)
{
    const std::size_t slot = Impl::slotOf(*view.archetype, component);
    if (slot == static_cast<std::size_t>(-1)) {
        return nullptr;
    }
    return Impl::arrayAt(*view.archetype, *view.chunk, slot);
}

std::size_t World::archetypeCount() const noexcept
{
    return impl_->archetypes.size();
}

std::size_t World::chunkCount() const noexcept
{
    std::size_t total = 0;
    for (const Archetype& archetype : impl_->archetypes) {
        total += archetype.chunks.size();
    }
    return total;
}

} // namespace harpia::ecs
