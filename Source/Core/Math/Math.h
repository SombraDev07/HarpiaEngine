// Harpia Engine — math
//
// glm does the linear algebra. Roadmap rule 7 — do not write what already
// exists — and the roadmap named glm for exactly this slot. What lives here is
// only the part glm has no opinion about: our depth convention, normal
// encoding, and the engine's own shapes.
//
// Conventions, once, for the whole engine:
//   - Right-handed world, +Y up. glm's default.
//   - Column-major storage, mul(matrix, vector). glm's default, and what HLSL
//     expects for a float4x4 in a structured buffer — the same bytes work on
//     both sides with no transpose on upload.
//   - Clip depth in [0, 1] (GLM_FORCE_DEPTH_ZERO_TO_ONE), Vulkan's range.
//   - REVERSE-Z: near maps to 1, far to 0, cleared to 0, tested with
//     GREATER_OR_EQUAL. Float precision clusters near zero, so this puts it
//     where the distant geometry is. glm has no reverse-Z helper; ours below.
//   - Vulkan clip space has +Y down, so our projections negate row 1. glm does
//     not do this for you.
#pragma once

#include <glm/glm.hpp>
#include <glm/gtc/matrix_inverse.hpp>
#include <glm/gtc/matrix_transform.hpp>
#include <glm/gtc/quaternion.hpp>

#include <array>
#include <cstddef>

namespace harpia {

// Engine-facing names. Everything else in the codebase spells vectors this way,
// so swapping the backing library later touches this block and nothing else.
using Vec2 = glm::vec2;
using Vec3 = glm::vec3;
using Vec4 = glm::vec4;
using IVec2 = glm::ivec2;
using UVec2 = glm::uvec2;
using Mat3 = glm::mat3;
using Mat4 = glm::mat4;
using Quat = glm::quat;

using glm::cross;
using glm::degrees;
using glm::distance;
using glm::dot;
using glm::inverse;
using glm::length;
using glm::mix;
using glm::normalize;
using glm::radians;
using glm::transpose;

inline constexpr float kPi = 3.14159265358979323846f;

// --- projections ------------------------------------------------------------

// Reverse-Z with an infinite far plane. Near maps to 1, infinity to 0.
[[nodiscard]] Mat4 perspectiveReverseZ(float fovYRadians, float aspect, float nearPlane) noexcept;

// Reverse-Z orthographic, for shadow cascades in F3.
[[nodiscard]] Mat4 orthographicReverseZ(float left, float right,
                                        float bottom, float top,
                                        float nearPlane, float farPlane) noexcept;

// Inverse transpose of the upper 3x3. Required whenever a model matrix carries
// non-uniform scale: transforming a normal by the model matrix would skew it.
[[nodiscard]] Mat3 normalMatrix(const Mat4& model) noexcept;

// --- normal encoding --------------------------------------------------------

// Octahedral: a unit vector in two channels, which is what the GBuffer stores.
// Unlike storing xy and reconstructing z, it covers both hemispheres.
[[nodiscard]] Vec2 encodeOctahedral(Vec3 normal) noexcept;
[[nodiscard]] Vec3 decodeOctahedral(Vec2 encoded) noexcept;

// --- transform --------------------------------------------------------------

// The shape an ECS transform component holds. Rotation is a quaternion because
// Euler angles gimbal-lock and interpolate along the wrong arc.
struct Transform {
    Vec3 position{0.0f};
    Quat rotation{1.0f, 0.0f, 0.0f, 0.0f}; // identity: w, x, y, z
    Vec3 scale{1.0f};

    [[nodiscard]] Mat4 matrix() const noexcept;
    [[nodiscard]] Vec3 forward() const noexcept;
    [[nodiscard]] Vec3 right() const noexcept;
    [[nodiscard]] Vec3 up() const noexcept;
};

// --- bounds -----------------------------------------------------------------

struct AABB {
    Vec3 min{ 1e30f};
    Vec3 max{-1e30f};

    void grow(Vec3 point) noexcept;
    void grow(const AABB& other) noexcept;

    [[nodiscard]] Vec3 centre() const noexcept;
    [[nodiscard]] Vec3 extent() const noexcept;
    [[nodiscard]] bool valid() const noexcept;
    [[nodiscard]] bool contains(Vec3 point) const noexcept;

    // The box that encloses this one after a transform. Uses the absolute
    // matrix trick rather than transforming all eight corners.
    [[nodiscard]] AABB transformed(const Mat4& matrix) const noexcept;
};

// --- frustum ----------------------------------------------------------------

struct Plane {
    Vec3  normal{0.0f, 1.0f, 0.0f};
    float distance = 0.0f;

    [[nodiscard]] float signedDistance(Vec3 point) const noexcept;
};

// Six planes with inward-facing normals, extracted from a view-projection.
class Frustum {
public:
    enum Side : std::size_t { Left, Right, Bottom, Top, Near, Far, Count };

    [[nodiscard]] static Frustum fromViewProjection(const Mat4& viewProjection) noexcept;

    // Conservative: a box straddling a plane counts as visible. False means
    // definitely outside, which is the only answer culling may act on.
    [[nodiscard]] bool intersects(const AABB& box) const noexcept;
    [[nodiscard]] bool contains(Vec3 point) const noexcept;

    [[nodiscard]] const Plane& plane(Side side) const noexcept
    {
        return planes_[static_cast<std::size_t>(side)];
    }

private:
    std::array<Plane, Count> planes_{};
};

} // namespace harpia
