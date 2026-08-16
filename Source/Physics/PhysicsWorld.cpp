#include "Physics/PhysicsWorld.h"

#include "Physics/JoltAllocator.h"
#include "Physics/JoltJobSystem.h"

#include "Core/Profiling/Profiler.h"

#include <Jolt/Core/Factory.h>
#include <Jolt/Core/TempAllocator.h>
#include <Jolt/Physics/Body/BodyCreationSettings.h>
#include <Jolt/Physics/Body/BodyLock.h>
#include <Jolt/Physics/Collision/CastResult.h>
#include <Jolt/Physics/Collision/RayCast.h>
#include <Jolt/Physics/Collision/Shape/BoxShape.h>
#include <Jolt/Physics/Collision/Shape/CapsuleShape.h>
#include <Jolt/Geometry/IndexedTriangle.h>
#include <Jolt/Physics/Collision/Shape/MeshShape.h>
#include <Jolt/Physics/Collision/Shape/RotatedTranslatedShape.h>
#include <Jolt/Physics/Collision/Shape/SphereShape.h>
#include <Jolt/RegisterTypes.h>

#include <mutex>
#include <utility>
#include <vector>

namespace harpia {
namespace {

namespace layers = phys::layers;

class ObjectLayerPairFilterImpl final : public JPH::ObjectLayerPairFilter {
public:
    bool ShouldCollide(JPH::ObjectLayer a, JPH::ObjectLayer b) const override
    {
        switch (a) {
            case layers::nonMoving: return b == layers::moving;
            case layers::moving:    return true;
            default:                return false;
        }
    }
};

class BPLayerInterfaceImpl final : public JPH::BroadPhaseLayerInterface {
public:
    BPLayerInterfaceImpl()
    {
        objectToBroadPhase_[layers::nonMoving] = layers::bpNonMoving;
        objectToBroadPhase_[layers::moving]    = layers::bpMoving;
    }

    JPH::uint GetNumBroadPhaseLayers() const override { return layers::bpCount; }

    JPH::BroadPhaseLayer GetBroadPhaseLayer(JPH::ObjectLayer layer) const override
    {
        return objectToBroadPhase_[layer];
    }

#if defined(JPH_EXTERNAL_PROFILE) || defined(JPH_PROFILE_ENABLED)
    const char* GetBroadPhaseLayerName(JPH::BroadPhaseLayer layer) const override
    {
        switch (static_cast<JPH::BroadPhaseLayer::Type>(layer)) {
            case static_cast<JPH::BroadPhaseLayer::Type>(layers::bpNonMoving): return "NON_MOVING";
            case static_cast<JPH::BroadPhaseLayer::Type>(layers::bpMoving):    return "MOVING";
            default: return "INVALID";
        }
    }
#endif

private:
    JPH::BroadPhaseLayer objectToBroadPhase_[layers::count]{};
};

class ObjectVsBroadPhaseLayerFilterImpl final : public JPH::ObjectVsBroadPhaseLayerFilter {
public:
    bool ShouldCollide(JPH::ObjectLayer object, JPH::BroadPhaseLayer broadPhase) const override
    {
        switch (object) {
            case layers::nonMoving: return broadPhase == layers::bpMoving;
            case layers::moving:    return true;
            default:                return false;
        }
    }
};

std::mutex g_joltMutex;
int        g_joltUsers = 0;

void retainJolt()
{
    const std::lock_guard lock(g_joltMutex);
    if (g_joltUsers++ == 0) {
        phys::installJoltAllocator();
        JPH::Factory::sInstance = new JPH::Factory();
        JPH::RegisterTypes();
    }
}

void releaseJolt()
{
    const std::lock_guard lock(g_joltMutex);
    if (--g_joltUsers == 0) {
        JPH::UnregisterTypes();
        delete JPH::Factory::sInstance;
        JPH::Factory::sInstance = nullptr;
    }
}

[[nodiscard]] JPH::ObjectLayer layerFor(JPH::EMotionType motion) noexcept
{
    return motion == JPH::EMotionType::Static ? layers::nonMoving : layers::moving;
}

[[nodiscard]] JPH::EMotionType toJolt(BodyMotion motion) noexcept
{
    switch (motion) {
        case BodyMotion::Static:    return JPH::EMotionType::Static;
        case BodyMotion::Kinematic: return JPH::EMotionType::Kinematic;
        case BodyMotion::Dynamic:   return JPH::EMotionType::Dynamic;
    }
    return JPH::EMotionType::Dynamic;
}

void applyMass(JPH::BodyCreationSettings& settings, JPH::EMotionType motion, float mass)
{
    if (motion != JPH::EMotionType::Dynamic) {
        return;
    }
    settings.mOverrideMassProperties = JPH::EOverrideMassProperties::CalculateInertia;
    settings.mMassPropertiesOverride.mMass = mass;
}

} // namespace

struct PhysicsWorld::Impl {
    explicit Impl(const Config& cfg)
        : config(cfg)
        , tempAllocator(cfg.tempAllocatorBytes)
        , jobSystem(2048, 8)
    {
        physics.Init(cfg.maxBodies, 0, cfg.maxBodyPairs, cfg.maxContactConstraints,
                     broadPhase, objectVsBroadPhase, objectVsObject);
        physics.SetGravity(phys::toJolt(cfg.gravity));
    }

    Config                            config;
    BPLayerInterfaceImpl              broadPhase;
    ObjectVsBroadPhaseLayerFilterImpl objectVsBroadPhase;
    ObjectLayerPairFilterImpl         objectVsObject;
    JPH::TempAllocatorImpl            tempAllocator;
    phys::JoltJobSystem               jobSystem;
    JPH::PhysicsSystem                physics;

    struct CharacterSlot {
        JPH::Ref<JPH::CharacterVirtual> character;
        std::uint32_t                   generation = 0;
        bool                            alive      = false;
        Vec3                            desiredVelocity{};
    };

    std::vector<CharacterSlot> characters;
    std::vector<std::uint32_t> freeCharacters;

    [[nodiscard]] JPH::BodyInterface&       bodies() { return physics.GetBodyInterface(); }
    [[nodiscard]] const JPH::BodyInterface& bodies() const { return physics.GetBodyInterface(); }

    [[nodiscard]] JPH::BodyID addShape(JPH::ShapeRefC shape, Vec3 position, Quat rotation,
                                       JPH::EMotionType motion, float mass,
                                       float friction, float restitution)
    {
        JPH::BodyCreationSettings settings(shape, phys::toJoltR(position), phys::toJolt(rotation),
                                           motion, layerFor(motion));
        settings.mFriction    = friction;
        settings.mRestitution = restitution;
        applyMass(settings, motion, mass);
        return bodies().CreateAndAddBody(settings, motion == JPH::EMotionType::Static
                                                       ? JPH::EActivation::DontActivate
                                                       : JPH::EActivation::Activate);
    }

    void updateCharacters(float dt)
    {
        const JPH::Vec3 gravity = physics.GetGravity();
        const JPH::CharacterVirtual::ExtendedUpdateSettings updateSettings;
        const auto bpFilter  = physics.GetDefaultBroadPhaseLayerFilter(layers::moving);
        const auto objFilter = physics.GetDefaultLayerFilter(layers::moving);

        for (CharacterSlot& slot : characters) {
            if (!slot.alive || slot.character == nullptr) {
                continue;
            }

            JPH::CharacterVirtual* character = slot.character.GetPtr();
            const JPH::Vec3 desired = phys::toJolt(slot.desiredVelocity);

            JPH::Vec3 velocity;
            if (character->GetGroundState() == JPH::CharacterVirtual::EGroundState::OnGround) {
                velocity = character->GetGroundVelocity() + desired;
            } else {
                const JPH::Vec3 current = character->GetLinearVelocity();
                velocity = JPH::Vec3(desired.GetX(), current.GetY(), desired.GetZ())
                           + gravity * dt;
            }

            character->SetLinearVelocity(velocity);
            character->ExtendedUpdate(dt, gravity, updateSettings,
                                      bpFilter, objFilter, {}, {}, tempAllocator);
        }
    }

    [[nodiscard]] CharacterSlot* slot(phys::CharacterHandle handle) noexcept
    {
        if (handle.index >= characters.size()) {
            return nullptr;
        }
        CharacterSlot& s = characters[handle.index];
        if (!s.alive || s.generation != handle.generation) {
            return nullptr;
        }
        return &s;
    }

    [[nodiscard]] const CharacterSlot* slot(phys::CharacterHandle handle) const noexcept
    {
        return const_cast<Impl*>(this)->slot(handle);
    }
};

PhysicsWorld::PhysicsWorld()
    : PhysicsWorld(Config{})
{
}

PhysicsWorld::PhysicsWorld(const Config& config)
{
    retainJolt();
    try {
        impl_ = std::make_unique<Impl>(config);
    } catch (...) {
        releaseJolt();
        throw;
    }
}

PhysicsWorld::~PhysicsWorld()
{
    if (!impl_) {
        return;
    }
    impl_->characters.clear();
    impl_.reset();
    releaseJolt();
}

void PhysicsWorld::step(float deltaTime, int collisionSteps)
{
    HARPIA_ZONE_NAMED("PhysicsWorld::step");
    impl_->physics.Update(deltaTime, collisionSteps, &impl_->tempAllocator, &impl_->jobSystem);
    impl_->updateCharacters(deltaTime);
}

void PhysicsWorld::step(ecs::World& world, float deltaTime, int collisionSteps)
{
    HARPIA_ZONE_NAMED("PhysicsWorld::step(World)");

    world.each<Transform, RigidBody>([this](ecs::Entity, Transform& transform, RigidBody& body) {
        if (!body.spawned) {
            JPH::Ref<JPH::Shape> shape;
            switch (body.shape) {
                case CollisionShape::Box:
                    shape = new JPH::BoxShape(JPH::Vec3(body.halfExtentX, body.halfExtentY,
                                                        body.halfExtentZ));
                    break;
                case CollisionShape::Sphere:
                    shape = new JPH::SphereShape(body.radius);
                    break;
                case CollisionShape::Capsule:
                    shape = new JPH::CapsuleShape(body.capsuleHalfHeight, body.radius);
                    break;
            }
            const JPH::BodyID id = impl_->addShape(shape, transform.position,
                                                   transform.rotation, toJolt(body.motion),
                                                   body.mass, body.friction, body.restitution);
            body.joltBodyId = id.GetIndexAndSequenceNumber();
            body.spawned    = true;
            return;
        }

        if (body.motion != BodyMotion::Dynamic) {
            impl_->bodies().SetPositionAndRotation(JPH::BodyID(body.joltBodyId),
                                                   phys::toJoltR(transform.position),
                                                   phys::toJolt(transform.rotation),
                                                   JPH::EActivation::DontActivate);
        }
    });

    world.each<Transform, CharacterController>([this](ecs::Entity, Transform& transform,
                                                      CharacterController& controller) {
        if (!controller.spawned) {
            const phys::CharacterHandle handle = addCharacter(transform.position,
                                                              controller.radius,
                                                              controller.height,
                                                              controller.maxSlopeDegrees);
            controller.characterIndex      = handle.index;
            controller.characterGeneration = handle.generation;
            controller.spawned             = handle.valid();
        }
        if (controller.spawned) {
            setCharacterVelocity({controller.characterIndex, controller.characterGeneration},
                                 Vec3(controller.desiredVelocityX,
                                      controller.desiredVelocityY,
                                      controller.desiredVelocityZ));
        }
    });

    step(deltaTime, collisionSteps);

    world.each<Transform, RigidBody>([this](ecs::Entity, Transform& transform, RigidBody& body) {
        if (!body.spawned || body.motion != BodyMotion::Dynamic) {
            return;
        }
        const JPH::BodyID id(body.joltBodyId);
        transform.position = bodyPosition(id);
        transform.rotation = bodyRotation(id);
    });

    world.each<Transform, CharacterController>([this](ecs::Entity, Transform& transform,
                                                      CharacterController& controller) {
        if (!controller.spawned) {
            return;
        }
        transform.position = characterPosition({controller.characterIndex,
                                                controller.characterGeneration});
    });
}

JPH::BodyID PhysicsWorld::addBox(Vec3 position, Vec3 halfExtent, JPH::EMotionType motion,
                                 float mass, float friction, float restitution)
{
    return impl_->addShape(JPH::ShapeRefC(new JPH::BoxShape(phys::toJolt(halfExtent))),
                           position, Quat{1.0f, 0.0f, 0.0f, 0.0f},
                           motion, mass, friction, restitution);
}

JPH::BodyID PhysicsWorld::addSphere(Vec3 position, float radius, JPH::EMotionType motion,
                                    float mass, float friction, float restitution)
{
    return impl_->addShape(JPH::ShapeRefC(new JPH::SphereShape(radius)),
                           position, Quat{1.0f, 0.0f, 0.0f, 0.0f},
                           motion, mass, friction, restitution);
}

JPH::BodyID PhysicsWorld::addCapsule(Vec3 position, float halfHeight, float radius,
                                     JPH::EMotionType motion,
                                     float mass, float friction, float restitution)
{
    return impl_->addShape(JPH::ShapeRefC(new JPH::CapsuleShape(halfHeight, radius)),
                           position, Quat{1.0f, 0.0f, 0.0f, 0.0f},
                           motion, mass, friction, restitution);
}

JPH::BodyID PhysicsWorld::addTriangleMesh(std::span<const Vec3>          vertices,
                                          std::span<const std::uint32_t> indices,
                                          Vec3 position, Quat rotation)
{
    JPH::VertexList          verts;
    JPH::IndexedTriangleList tris;
    verts.reserve(vertices.size());
    for (const Vec3& vertex : vertices) {
        verts.push_back(JPH::Float3(vertex.x, vertex.y, vertex.z));
    }
    tris.reserve(indices.size() / 3);
    for (std::size_t i = 0; i + 2 < indices.size(); i += 3) {
        const std::uint32_t a = indices[i];
        const std::uint32_t b = indices[i + 1];
        const std::uint32_t c = indices[i + 2];
        if (a >= vertices.size() || b >= vertices.size() || c >= vertices.size()) {
            continue;
        }
        tris.push_back(JPH::IndexedTriangle(a, b, c, 0));
    }

    JPH::MeshShapeSettings meshSettings(verts, tris);
    meshSettings.SetEmbedded();
    const JPH::Shape::ShapeResult result = meshSettings.Create();
    if (result.HasError() || result.Get() == nullptr) {
        return JPH::BodyID();
    }

    const JPH::BodyID id = impl_->addShape(result.Get(), position, rotation,
                                           JPH::EMotionType::Static, 0.0f, 0.5f, 0.0f);
    impl_->physics.OptimizeBroadPhase();
    return id;
}

JPH::BodyID PhysicsWorld::addCollisionMesh(const MeshAsset& mesh, Vec3 position, Quat rotation)
{
    std::vector<Vec3>          positions;
    std::vector<std::uint32_t> indices;
    positions.reserve(mesh.vertices.size());
    for (const MeshVertex& vertex : mesh.vertices) {
        positions.push_back(vertex.position);
    }

    if (mesh.subMeshes.empty()) {
        return addTriangleMesh(positions, mesh.indices, position, rotation);
    }

    indices.reserve(mesh.indices.size());
    for (const SubMesh& sub : mesh.subMeshes) {
        for (std::uint32_t i = 0; i < sub.indexCount; ++i) {
            indices.push_back(mesh.indices[sub.firstIndex + i] + sub.vertexOffset);
        }
    }
    return addTriangleMesh(positions, indices, position, rotation);
}

void PhysicsWorld::removeBody(JPH::BodyID id)
{
    if (id.IsInvalid()) {
        return;
    }
    JPH::BodyInterface& iface = impl_->bodies();
    if (iface.IsAdded(id)) {
        iface.RemoveBody(id);
    }
    iface.DestroyBody(id);
}

phys::CharacterHandle PhysicsWorld::addCharacter(Vec3 feetPosition, float radius,
                                                 float height, float maxSlopeDegrees)
{
    if (height <= radius * 2.0f) {
        height = radius * 2.0f + 0.1f;
    }

    const float halfCylinder = 0.5f * height - radius;
    JPH::RefConst<JPH::Shape> capsule = new JPH::CapsuleShape(halfCylinder, radius);
    JPH::RotatedTranslatedShapeSettings offset(
        JPH::Vec3(0.0f, halfCylinder + radius, 0.0f), JPH::Quat::sIdentity(), capsule);
    offset.SetEmbedded();
    const JPH::Shape::ShapeResult offsetResult = offset.Create();
    if (offsetResult.HasError()) {
        return {};
    }

    JPH::Ref<JPH::CharacterVirtualSettings> settings = new JPH::CharacterVirtualSettings();
    settings->mMaxSlopeAngle = JPH::DegreesToRadians(maxSlopeDegrees);
    settings->mShape = offsetResult.Get();
    settings->mSupportingVolume = JPH::Plane(JPH::Vec3::sAxisY(), -radius);

    JPH::Ref<JPH::CharacterVirtual> character = new JPH::CharacterVirtual(
        settings, phys::toJoltR(feetPosition), JPH::Quat::sIdentity(), &impl_->physics);

    std::uint32_t index = 0;
    if (!impl_->freeCharacters.empty()) {
        index = impl_->freeCharacters.back();
        impl_->freeCharacters.pop_back();
    } else {
        index = static_cast<std::uint32_t>(impl_->characters.size());
        impl_->characters.emplace_back();
    }

    Impl::CharacterSlot& slot = impl_->characters[index];
    slot.character = std::move(character);
    slot.generation = slot.generation == 0 ? 1 : slot.generation + 1;
    if ((slot.generation & 1u) == 0) {
        ++slot.generation;
    }
    slot.alive = true;
    slot.desiredVelocity = {};
    return {index, slot.generation};
}

void PhysicsWorld::removeCharacter(phys::CharacterHandle handle)
{
    Impl::CharacterSlot* slot = impl_->slot(handle);
    if (slot == nullptr) {
        return;
    }
    slot->character = nullptr;
    slot->alive     = false;
    ++slot->generation;
    impl_->freeCharacters.push_back(handle.index);
}

void PhysicsWorld::setCharacterVelocity(phys::CharacterHandle handle, Vec3 velocity)
{
    if (Impl::CharacterSlot* slot = impl_->slot(handle)) {
        slot->desiredVelocity = velocity;
    }
}

Vec3 PhysicsWorld::characterPosition(phys::CharacterHandle handle) const
{
    const Impl::CharacterSlot* slot = impl_->slot(handle);
    if (slot == nullptr || slot->character == nullptr) {
        return {};
    }
    return phys::fromJoltR(slot->character->GetPosition());
}

JPH::CharacterVirtual* PhysicsWorld::character(phys::CharacterHandle handle) noexcept
{
    Impl::CharacterSlot* slot = impl_->slot(handle);
    return slot != nullptr ? slot->character.GetPtr() : nullptr;
}

const JPH::CharacterVirtual* PhysicsWorld::character(phys::CharacterHandle handle) const noexcept
{
    const Impl::CharacterSlot* slot = impl_->slot(handle);
    return slot != nullptr ? slot->character.GetPtr() : nullptr;
}

phys::RayHit PhysicsWorld::raycast(Vec3 origin, Vec3 direction, float maxDistance) const
{
    phys::RayHit hit;
    const float length = glm::length(direction);
    if (length < 1e-8f || maxDistance <= 0.0f) {
        return hit;
    }

    const Vec3 unit = direction / length;
    const JPH::RRayCast ray{phys::toJoltR(origin), phys::toJolt(unit) * maxDistance};
    JPH::RayCastResult result;
    if (!impl_->physics.GetNarrowPhaseQuery().CastRay(ray, result)) {
        return hit;
    }

    hit.hit      = true;
    hit.fraction = result.mFraction;
    hit.bodyId   = result.mBodyID;
    hit.point    = origin + unit * (maxDistance * result.mFraction);

    const JPH::BodyLockRead lock(impl_->physics.GetBodyLockInterface(), result.mBodyID);
    if (lock.Succeeded()) {
        hit.normal = phys::fromJolt(lock.GetBody().GetWorldSpaceSurfaceNormal(
            result.mSubShapeID2, phys::toJoltR(hit.point)));
    }
    return hit;
}

JPH::PhysicsSystem& PhysicsWorld::system() noexcept
{
    return impl_->physics;
}

const JPH::PhysicsSystem& PhysicsWorld::system() const noexcept
{
    return impl_->physics;
}

JPH::BodyInterface& PhysicsWorld::bodies() noexcept
{
    return impl_->bodies();
}

const JPH::BodyInterface& PhysicsWorld::bodies() const noexcept
{
    return impl_->bodies();
}

Vec3 PhysicsWorld::bodyPosition(JPH::BodyID id) const
{
    return phys::fromJoltR(impl_->bodies().GetPosition(id));
}

Quat PhysicsWorld::bodyRotation(JPH::BodyID id) const
{
    return phys::fromJolt(impl_->bodies().GetRotation(id));
}

} // namespace harpia
