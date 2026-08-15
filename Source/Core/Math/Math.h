// Harpia Engine — math
//
// Scalar and plain, on purpose. The roadmap lists math as "own SIMD library or
// glm at first — swappable later"; what matters now is that the conventions are
// fixed and documented, because a silent convention mismatch between C++ and
// HLSL is the hardest kind of rendering bug to find.
//
// Conventions, once, for the whole engine:
//   - Right-handed world, +Y up.
//   - Matrices are COLUMN-major in memory: m[column * 4 + row]. That is what
//     HLSL expects by default for a float4x4 in a structured buffer, so the
//     same bytes work on both sides with no transpose on upload.
//   - Transforms apply as mul(matrix, vector) — column vector on the right.
//   - Depth is REVERSE-Z: near maps to 1, far to 0, cleared to 0 and tested
//     with GREATER_OR_EQUAL. Float precision clusters near 0, so this puts the
//     precision where the distant geometry is.
#pragma once

#include <cmath>
#include <cstddef>

namespace harpia {

struct Vec2 {
    float x = 0.0f;
    float y = 0.0f;
};

struct Vec3 {
    float x = 0.0f;
    float y = 0.0f;
    float z = 0.0f;
};

struct Vec4 {
    float x = 0.0f;
    float y = 0.0f;
    float z = 0.0f;
    float w = 0.0f;
};

[[nodiscard]] constexpr Vec3 operator+(Vec3 a, Vec3 b) noexcept
{
    return Vec3{a.x + b.x, a.y + b.y, a.z + b.z};
}

[[nodiscard]] constexpr Vec3 operator-(Vec3 a, Vec3 b) noexcept
{
    return Vec3{a.x - b.x, a.y - b.y, a.z - b.z};
}

[[nodiscard]] constexpr Vec3 operator*(Vec3 v, float s) noexcept
{
    return Vec3{v.x * s, v.y * s, v.z * s};
}

[[nodiscard]] constexpr float dot(Vec3 a, Vec3 b) noexcept
{
    return a.x * b.x + a.y * b.y + a.z * b.z;
}

[[nodiscard]] constexpr Vec3 cross(Vec3 a, Vec3 b) noexcept
{
    return Vec3{a.y * b.z - a.z * b.y,
                a.z * b.x - a.x * b.z,
                a.x * b.y - a.y * b.x};
}

[[nodiscard]] inline float length(Vec3 v) noexcept
{
    return std::sqrt(dot(v, v));
}

[[nodiscard]] inline Vec3 normalize(Vec3 v) noexcept
{
    const float len = length(v);
    return len > 1e-8f ? v * (1.0f / len) : Vec3{};
}

// Column-major 4x4. Element (row, column) lives at m[column * 4 + row].
struct Mat4 {
    float m[16]{1, 0, 0, 0,
                0, 1, 0, 0,
                0, 0, 1, 0,
                0, 0, 0, 1};

    [[nodiscard]] float& at(std::size_t row, std::size_t column) noexcept
    {
        return m[column * 4 + row];
    }

    [[nodiscard]] float at(std::size_t row, std::size_t column) const noexcept
    {
        return m[column * 4 + row];
    }

    [[nodiscard]] static Mat4 identity() noexcept { return Mat4{}; }
    [[nodiscard]] static Mat4 translation(Vec3 offset) noexcept;
    [[nodiscard]] static Mat4 scale(Vec3 factors) noexcept;
    [[nodiscard]] static Mat4 rotationX(float radians) noexcept;
    [[nodiscard]] static Mat4 rotationY(float radians) noexcept;
    [[nodiscard]] static Mat4 rotationZ(float radians) noexcept;

    // Right-handed look-at.
    [[nodiscard]] static Mat4 lookAt(Vec3 eye, Vec3 target, Vec3 up) noexcept;

    // Reverse-Z, infinite far plane, Vulkan clip space (Y down, Z in [0,1]).
    // Near maps to 1 and infinity to 0.
    [[nodiscard]] static Mat4 perspectiveReverseZ(float fovYRadians,
                                                  float aspect,
                                                  float nearPlane) noexcept;

    [[nodiscard]] Mat4 transposed() const noexcept;
};

[[nodiscard]] Mat4 operator*(const Mat4& a, const Mat4& b) noexcept;
[[nodiscard]] Vec4 operator*(const Mat4& m, const Vec4& v) noexcept;

[[nodiscard]] Vec3 transformPoint(const Mat4& m, Vec3 point) noexcept;
[[nodiscard]] Vec3 transformDirection(const Mat4& m, Vec3 direction) noexcept;

// Octahedral normal encoding: a unit vector in two channels, which is what the
// GBuffer stores instead of three. Error is well under a degree, and unlike
// storing xy and reconstructing z it handles both hemispheres.
[[nodiscard]] Vec2 encodeOctahedral(Vec3 normal) noexcept;
[[nodiscard]] Vec3 decodeOctahedral(Vec2 encoded) noexcept;

inline constexpr float kPi = 3.14159265358979323846f;

[[nodiscard]] constexpr float radians(float degrees) noexcept
{
    return degrees * (kPi / 180.0f);
}

} // namespace harpia
