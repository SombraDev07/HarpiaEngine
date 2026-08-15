// glm does the linear algebra, so these tests do not re-test glm. They pin the
// conventions the engine layers on top: Vulkan depth range, reverse-Z, the Y
// flip, and our own encoding and shapes. A silent convention drift here is the
// hardest rendering bug to find, which is why it gets numbers rather than trust.

#include <doctest/doctest.h>

#include "Core/Math/Math.h"

#include <glm/gtc/type_ptr.hpp>

#include <cmath>
#include <initializer_list>

using namespace harpia;

namespace {

void checkVec3(Vec3 actual, Vec3 expected, float epsilon = 1e-5f)
{
    CHECK(actual.x == doctest::Approx(expected.x).epsilon(epsilon));
    CHECK(actual.y == doctest::Approx(expected.y).epsilon(epsilon));
    CHECK(actual.z == doctest::Approx(expected.z).epsilon(epsilon));
}

} // namespace

TEST_CASE("matrix memory layout is column-major")
{
    const Mat4 m = glm::translate(Mat4(1.0f), Vec3(7.0f, 8.0f, 9.0f));

    // Translation lands at flat indices 12..14. That is what HLSL expects to
    // find for a float4x4 in a structured buffer, so the same bytes serve both
    // sides with no transpose on upload.
    const float* flat = glm::value_ptr(m);
    CHECK(flat[12] == doctest::Approx(7.0f));
    CHECK(flat[13] == doctest::Approx(8.0f));
    CHECK(flat[14] == doctest::Approx(9.0f));
    CHECK(flat[3] == doctest::Approx(0.0f));
}

TEST_CASE("the world is right-handed")
{
    // A quarter turn about +Y takes +X to -Z. Getting this backwards mirrors
    // every model in the scene.
    const Mat4 rotate = glm::rotate(Mat4(1.0f), radians(90.0f), Vec3(0, 1, 0));
    checkVec3(Vec3(rotate * Vec4(1, 0, 0, 1)), Vec3(0, 0, -1));

    checkVec3(cross(Vec3(1, 0, 0), Vec3(0, 1, 0)), Vec3(0, 0, 1));
}

TEST_CASE("lookAt puts the target down -Z in view space")
{
    const Mat4 view = glm::lookAt(Vec3(0, 0, 5), Vec3(0, 0, 0), Vec3(0, 1, 0));

    checkVec3(Vec3(view * Vec4(0, 0, 0, 1)), Vec3(0, 0, -5));
    checkVec3(Vec3(view * Vec4(0, 0, 5, 1)), Vec3(0, 0, 0));
}

TEST_CASE("reverse-Z perspective maps near to 1 and distance towards 0")
{
    constexpr float kNear = 0.1f;
    const Mat4 projection = perspectiveReverseZ(radians(60.0f), 16.0f / 9.0f, kNear);

    const auto depthAt = [&](float viewZ) {
        const Vec4 clip = projection * Vec4(0.0f, 0.0f, viewZ, 1.0f);
        return clip.z / clip.w;
    };

    CHECK(depthAt(-kNear) == doctest::Approx(1.0f));
    CHECK(depthAt(-1.0f) == doctest::Approx(0.1f));
    CHECK(depthAt(-1000.0f) == doctest::Approx(0.0001f));

    // Monotonically decreasing with distance. This is why the pipeline clears
    // depth to 0 and tests GREATER_OR_EQUAL.
    CHECK(depthAt(-1.0f) > depthAt(-10.0f));
    CHECK(depthAt(-10.0f) > depthAt(-100.0f));
}

TEST_CASE("projections flip Y for Vulkan clip space")
{
    const Mat4 perspective = perspectiveReverseZ(radians(90.0f), 1.0f, 0.1f);

    // A point above the axis must land at negative clip Y: Vulkan's +Y is down.
    CHECK((perspective * Vec4(0.0f, 1.0f, -1.0f, 1.0f)).y < 0.0f);
    CHECK((perspective * Vec4(1.0f, 0.0f, -1.0f, 1.0f)).x > 0.0f);

    const Mat4 ortho = orthographicReverseZ(-1, 1, -1, 1, 0.1f, 100.0f);
    CHECK((ortho * Vec4(0.0f, 1.0f, -1.0f, 1.0f)).y < 0.0f);
}

TEST_CASE("reverse-Z orthographic maps near to 1 and far to 0")
{
    const Mat4 ortho = orthographicReverseZ(-10, 10, -10, 10, 1.0f, 100.0f);

    const auto depthAt = [&](float viewZ) {
        const Vec4 clip = ortho * Vec4(0.0f, 0.0f, viewZ, 1.0f);
        return clip.z / clip.w;
    };

    CHECK(depthAt(-1.0f) == doctest::Approx(1.0f));    // near
    CHECK(depthAt(-100.0f) == doctest::Approx(0.0f));  // far
    CHECK(depthAt(-50.0f) > depthAt(-51.0f));
}

TEST_CASE("a point inside the frustum lands inside the clip cube")
{
    const Mat4 view = glm::lookAt(Vec3(0, 0, 5), Vec3(0, 0, 0), Vec3(0, 1, 0));
    const Mat4 viewProjection = perspectiveReverseZ(radians(60.0f), 1.0f, 0.1f) * view;

    const Vec4 clip = viewProjection * Vec4(0, 0, 0, 1);
    REQUIRE(clip.w > 0.0f);

    const Vec3 ndc = Vec3(clip) / clip.w;
    CHECK(ndc.x == doctest::Approx(0.0f).epsilon(1e-5));
    CHECK(ndc.y == doctest::Approx(0.0f).epsilon(1e-5));
    CHECK(ndc.z > 0.0f);
    CHECK(ndc.z < 1.0f);
}

TEST_CASE("the normal matrix survives non-uniform scale")
{
    // Squashing Y by 4 tilts a diagonal normal; the model matrix alone would
    // tilt it the wrong way, which is the classic lighting-is-subtly-wrong bug.
    const Mat4 model = glm::scale(Mat4(1.0f), Vec3(1.0f, 0.25f, 1.0f));
    const Vec3 normal = normalize(Vec3(0.0f, 1.0f, 1.0f));

    const Vec3 wrong = normalize(Vec3(model * Vec4(normal, 0.0f)));
    const Vec3 right = normalize(normalMatrix(model) * normal);

    // A squashed surface tips its normal towards the squashed axis, not away.
    CHECK(right.y > wrong.y);

    // A plane's normal must stay perpendicular to a tangent on that plane.
    const Vec3 tangent = normalize(Vec3(0.0f, 1.0f, -1.0f));
    const Vec3 tangentAfter = normalize(Vec3(model * Vec4(tangent, 0.0f)));
    CHECK(dot(right, tangentAfter) == doctest::Approx(0.0f).epsilon(1e-4));
}

TEST_CASE("octahedral encoding round trips over the whole sphere")
{
    float worstError = 0.0f;

    for (int i = 0; i < 64; ++i) {
        for (int j = 0; j < 64; ++j) {
            const float theta = kPi * (static_cast<float>(i) + 0.5f) / 64.0f;
            const float phi   = 2.0f * kPi * (static_cast<float>(j) + 0.5f) / 64.0f;

            const Vec3 normal{std::sin(theta) * std::cos(phi),
                              std::sin(theta) * std::sin(phi),
                              std::cos(theta)};

            const float error = length(decodeOctahedral(encodeOctahedral(normal)) - normal);
            worstError = error > worstError ? error : worstError;
        }
    }

    // Anything above this would show as banding in specular highlights.
    CHECK(worstError < 1e-3f);
}

TEST_CASE("octahedral encoding stays inside the storable range")
{
    for (const Vec3 axis : {Vec3(1, 0, 0), Vec3(-1, 0, 0), Vec3(0, 1, 0),
                            Vec3(0, -1, 0), Vec3(0, 0, 1), Vec3(0, 0, -1)}) {
        const Vec2 encoded = encodeOctahedral(axis);
        CHECK(encoded.x >= -1.0f);
        CHECK(encoded.x <= 1.0f);
        CHECK(encoded.y >= -1.0f);
        CHECK(encoded.y <= 1.0f);
        checkVec3(decodeOctahedral(encoded), axis, 1e-4f);
    }

    // A zero vector must not produce NaN.
    const Vec3 degenerate = decodeOctahedral(encodeOctahedral(Vec3(0.0f)));
    CHECK(std::isfinite(degenerate.x));
    CHECK(std::isfinite(degenerate.y));
    CHECK(std::isfinite(degenerate.z));
}

TEST_CASE("Transform composes translation, rotation and scale in that order")
{
    Transform transform;
    transform.position = Vec3(10, 0, 0);
    transform.rotation = glm::angleAxis(radians(90.0f), Vec3(0, 1, 0));
    transform.scale    = Vec3(2.0f);

    // Scale first, then rotate, then translate: +X scaled to 2, rotated to -Z,
    // then moved by the position.
    checkVec3(Vec3(transform.matrix() * Vec4(1, 0, 0, 1)), Vec3(10, 0, -2));

    // A default transform is the identity.
    checkVec3(Vec3(Transform{}.matrix() * Vec4(3, 4, 5, 1)), Vec3(3, 4, 5));
}

TEST_CASE("Transform basis vectors follow the right-handed convention")
{
    Transform transform;
    checkVec3(transform.forward(), Vec3(0, 0, -1)); // looks down its own -Z
    checkVec3(transform.right(), Vec3(1, 0, 0));
    checkVec3(transform.up(), Vec3(0, 1, 0));

    transform.rotation = glm::angleAxis(radians(90.0f), Vec3(0, 1, 0));
    checkVec3(transform.forward(), Vec3(-1, 0, 0));
}

TEST_CASE("AABB grows, measures and reports validity")
{
    AABB box;
    CHECK_FALSE(box.valid()); // default is the empty box

    box.grow(Vec3(-1, -2, -3));
    box.grow(Vec3(4, 5, 6));

    CHECK(box.valid());
    checkVec3(box.centre(), Vec3(1.5f, 1.5f, 1.5f));
    checkVec3(box.extent(), Vec3(2.5f, 3.5f, 4.5f));
    CHECK(box.contains(Vec3(0, 0, 0)));
    CHECK_FALSE(box.contains(Vec3(10, 0, 0)));

    AABB other;
    other.grow(Vec3(-10, 0, 0));
    box.grow(other);
    CHECK(box.min.x == doctest::Approx(-10.0f));
}

TEST_CASE("a transformed AABB encloses the transformed corners")
{
    AABB box;
    box.grow(Vec3(-1, -1, -1));
    box.grow(Vec3(1, 1, 1));

    const Mat4 matrix = glm::translate(Mat4(1.0f), Vec3(10, 0, 0))
                      * glm::rotate(Mat4(1.0f), radians(45.0f), Vec3(0, 0, 1));

    const AABB transformed = box.transformed(matrix);

    // Every original corner must land inside the new box.
    for (int i = 0; i < 8; ++i) {
        const Vec3 corner{(i & 1) ? box.max.x : box.min.x,
                          (i & 2) ? box.max.y : box.min.y,
                          (i & 4) ? box.max.z : box.min.z};
        const Vec3 moved = Vec3(matrix * Vec4(corner, 1.0f));

        // Corners land exactly on the boundary of a tight box, so the
        // tolerance has to widen the box rather than move the point.
        constexpr float kEpsilon = 1e-4f;
        CHECK(moved.x >= transformed.min.x - kEpsilon);
        CHECK(moved.y >= transformed.min.y - kEpsilon);
        CHECK(moved.z >= transformed.min.z - kEpsilon);
        CHECK(moved.x <= transformed.max.x + kEpsilon);
        CHECK(moved.y <= transformed.max.y + kEpsilon);
        CHECK(moved.z <= transformed.max.z + kEpsilon);
    }

    // A 45 degree turn widens the box by sqrt(2) on the rotated axes.
    CHECK(transformed.extent().x == doctest::Approx(std::sqrt(2.0f)).epsilon(1e-4));
    CHECK(transformed.extent().z == doctest::Approx(1.0f).epsilon(1e-4));
}

TEST_CASE("the frustum keeps what is visible and drops what is not")
{
    const Mat4 view = glm::lookAt(Vec3(0, 0, 5), Vec3(0, 0, 0), Vec3(0, 1, 0));
    const Mat4 viewProjection = perspectiveReverseZ(radians(60.0f), 1.0f, 0.1f) * view;
    const Frustum frustum = Frustum::fromViewProjection(viewProjection);

    CHECK(frustum.contains(Vec3(0, 0, 0)));      // straight ahead
    CHECK_FALSE(frustum.contains(Vec3(0, 0, 10))); // behind the camera
    CHECK_FALSE(frustum.contains(Vec3(50, 0, 0))); // far off to the side

    AABB inside;
    inside.grow(Vec3(-0.5f));
    inside.grow(Vec3(0.5f));
    CHECK(frustum.intersects(inside));

    AABB behind;
    behind.grow(Vec3(-1, -1, 20));
    behind.grow(Vec3(1, 1, 22));
    CHECK_FALSE(frustum.intersects(behind));

    AABB offToTheSide;
    offToTheSide.grow(Vec3(100, -1, -1));
    offToTheSide.grow(Vec3(102, 1, 1));
    CHECK_FALSE(frustum.intersects(offToTheSide));
}

TEST_CASE("frustum culling is conservative at the boundary")
{
    const Mat4 view = glm::lookAt(Vec3(0, 0, 5), Vec3(0, 0, 0), Vec3(0, 1, 0));
    const Mat4 viewProjection = perspectiveReverseZ(radians(60.0f), 1.0f, 0.1f) * view;
    const Frustum frustum = Frustum::fromViewProjection(viewProjection);

    // A box whose centre is outside but which straddles the edge must be kept:
    // dropping it would pop geometry at the screen border.
    AABB straddling;
    straddling.grow(Vec3(2.0f, -1.0f, -1.0f));
    straddling.grow(Vec3(6.0f, 1.0f, 1.0f));
    CHECK(frustum.intersects(straddling));

    // An invalid box is never visible.
    CHECK_FALSE(frustum.intersects(AABB{}));
}
