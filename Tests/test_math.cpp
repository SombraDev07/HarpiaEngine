// Conventions get pinned here rather than discovered later from a wrong image.
// Column-major storage, mul(matrix, vector), right-handed world, reverse-Z.

#include <doctest/doctest.h>

#include "Core/Math/Math.h"

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

TEST_CASE("storage is column-major: element (row, col) is at col * 4 + row")
{
    Mat4 m;
    m.at(0, 3) = 7.0f; // row 0, column 3 — the translation X slot

    // Column-major means the translation lands at index 12, which is what a
    // float4x4 in HLSL expects to find there.
    CHECK(m.m[12] == doctest::Approx(7.0f));
    CHECK(m.m[3] == doctest::Approx(0.0f));
}

TEST_CASE("identity leaves a point alone")
{
    checkVec3(transformPoint(Mat4::identity(), Vec3{1, 2, 3}), Vec3{1, 2, 3});
}

TEST_CASE("translation moves points and not directions")
{
    const Mat4 m = Mat4::translation(Vec3{10, 20, 30});

    checkVec3(transformPoint(m, Vec3{1, 2, 3}), Vec3{11, 22, 33});
    // A direction has w = 0, so translation must not touch it.
    checkVec3(transformDirection(m, Vec3{1, 0, 0}), Vec3{1, 0, 0});
}

TEST_CASE("scale and rotation do what their names say")
{
    checkVec3(transformPoint(Mat4::scale(Vec3{2, 3, 4}), Vec3{1, 1, 1}), Vec3{2, 3, 4});

    // A quarter turn about Y takes +X to -Z in a right-handed world.
    checkVec3(transformPoint(Mat4::rotationY(radians(90.0f)), Vec3{1, 0, 0}),
              Vec3{0, 0, -1});

    // About Z, +X goes to +Y.
    checkVec3(transformPoint(Mat4::rotationZ(radians(90.0f)), Vec3{1, 0, 0}),
              Vec3{0, 1, 0});

    // About X, +Y goes to +Z.
    checkVec3(transformPoint(Mat4::rotationX(radians(90.0f)), Vec3{0, 1, 0}),
              Vec3{0, 0, 1});
}

TEST_CASE("multiplication applies the right-hand matrix first")
{
    const Mat4 translate = Mat4::translation(Vec3{10, 0, 0});
    const Mat4 scale     = Mat4::scale(Vec3{2, 2, 2});

    // translate * scale: scale runs first, so the point is scaled then moved.
    checkVec3(transformPoint(translate * scale, Vec3{1, 0, 0}), Vec3{12, 0, 0});
    // scale * translate: moved then scaled.
    checkVec3(transformPoint(scale * translate, Vec3{1, 0, 0}), Vec3{22, 0, 0});
}

TEST_CASE("lookAt puts the target down -Z in view space")
{
    const Mat4 view = Mat4::lookAt(Vec3{0, 0, 5}, Vec3{0, 0, 0}, Vec3{0, 1, 0});

    // The camera sits 5 units away, so the target lands at -5 on the view Z.
    checkVec3(transformPoint(view, Vec3{0, 0, 0}), Vec3{0, 0, -5});
    // The eye itself lands at the view-space origin.
    checkVec3(transformPoint(view, Vec3{0, 0, 5}), Vec3{0, 0, 0});
    // World +X stays view +X for this orientation.
    checkVec3(transformPoint(view, Vec3{1, 0, 0}), Vec3{1, 0, -5});
}

TEST_CASE("lookAt from an angle keeps the target centred")
{
    const Mat4 view = Mat4::lookAt(Vec3{3, 4, 5}, Vec3{0, 0, 0}, Vec3{0, 1, 0});
    const Vec3 target = transformPoint(view, Vec3{0, 0, 0});

    // Centred means x and y are zero; only the distance survives, on -Z.
    CHECK(target.x == doctest::Approx(0.0f).epsilon(1e-4));
    CHECK(target.y == doctest::Approx(0.0f).epsilon(1e-4));
    CHECK(target.z == doctest::Approx(-std::sqrt(9.0f + 16.0f + 25.0f)).epsilon(1e-4));
}

TEST_CASE("reverse-Z projection maps near to 1 and distance towards 0")
{
    constexpr float kNear = 0.1f;
    const Mat4 projection = Mat4::perspectiveReverseZ(radians(60.0f), 16.0f / 9.0f, kNear);

    const auto depthAt = [&](float viewZ) {
        // View space looks down -Z, so a point in front has negative z.
        const Vec4 clip = projection * Vec4{0.0f, 0.0f, viewZ, 1.0f};
        return clip.z / clip.w;
    };

    CHECK(depthAt(-kNear) == doctest::Approx(1.0f));       // near plane
    CHECK(depthAt(-1.0f) == doctest::Approx(0.1f));        // near / distance
    CHECK(depthAt(-1000.0f) == doctest::Approx(0.0001f));  // approaching zero

    // Monotonically decreasing with distance — that is the whole point, and it
    // is why the pipeline tests depth with GREATER_OR_EQUAL and clears to 0.
    CHECK(depthAt(-1.0f) > depthAt(-10.0f));
    CHECK(depthAt(-10.0f) > depthAt(-100.0f));
}

TEST_CASE("projection flips Y for Vulkan clip space")
{
    const Mat4 projection = Mat4::perspectiveReverseZ(radians(90.0f), 1.0f, 0.1f);

    // A point above the camera axis must land at negative clip Y, because
    // Vulkan's +Y points down the screen.
    const Vec4 clip = projection * Vec4{0.0f, 1.0f, -1.0f, 1.0f};
    CHECK(clip.y < 0.0f);

    // A point to the right stays on the right.
    const Vec4 right = projection * Vec4{1.0f, 0.0f, -1.0f, 1.0f};
    CHECK(right.x > 0.0f);
}

TEST_CASE("a point inside the frustum lands inside the clip cube")
{
    const Mat4 view = Mat4::lookAt(Vec3{0, 0, 5}, Vec3{0, 0, 0}, Vec3{0, 1, 0});
    const Mat4 projection = Mat4::perspectiveReverseZ(radians(60.0f), 1.0f, 0.1f);
    const Mat4 viewProjection = projection * view;

    const Vec4 clip = viewProjection * Vec4{0, 0, 0, 1};
    REQUIRE(clip.w > 0.0f);

    const Vec3 ndc{clip.x / clip.w, clip.y / clip.w, clip.z / clip.w};
    CHECK(ndc.x == doctest::Approx(0.0f).epsilon(1e-5));
    CHECK(ndc.y == doctest::Approx(0.0f).epsilon(1e-5));
    CHECK(ndc.z > 0.0f);
    CHECK(ndc.z < 1.0f);
}

TEST_CASE("octahedral encoding round trips over the whole sphere")
{
    float worstError = 0.0f;

    // Sweep both hemispheres; the lower one is what a naive xy-only encoding
    // gets wrong.
    for (int i = 0; i < 64; ++i) {
        for (int j = 0; j < 64; ++j) {
            const float theta = kPi * (static_cast<float>(i) + 0.5f) / 64.0f;
            const float phi   = 2.0f * kPi * (static_cast<float>(j) + 0.5f) / 64.0f;

            const Vec3 normal{std::sin(theta) * std::cos(phi),
                              std::sin(theta) * std::sin(phi),
                              std::cos(theta)};

            const Vec3 decoded = decodeOctahedral(encodeOctahedral(normal));
            const float error  = length(decoded - normal);
            worstError = error > worstError ? error : worstError;
        }
    }

    // Two channels of float precision; anything above this would show as
    // visible banding in specular highlights.
    CHECK(worstError < 1e-3f);
}

TEST_CASE("octahedral encoding stays inside the storable range")
{
    for (const Vec3 axis : {Vec3{1, 0, 0}, Vec3{-1, 0, 0}, Vec3{0, 1, 0},
                            Vec3{0, -1, 0}, Vec3{0, 0, 1}, Vec3{0, 0, -1}}) {
        const Vec2 encoded = encodeOctahedral(axis);
        CHECK(encoded.x >= -1.0f);
        CHECK(encoded.x <= 1.0f);
        CHECK(encoded.y >= -1.0f);
        CHECK(encoded.y <= 1.0f);

        checkVec3(decodeOctahedral(encoded), axis, 1e-4f);
    }
}

TEST_CASE("degenerate input does not produce NaN")
{
    const Vec3 decoded = decodeOctahedral(encodeOctahedral(Vec3{0, 0, 0}));
    CHECK(std::isfinite(decoded.x));
    CHECK(std::isfinite(decoded.y));
    CHECK(std::isfinite(decoded.z));

    CHECK(length(normalize(Vec3{0, 0, 0})) == doctest::Approx(0.0f));
}
