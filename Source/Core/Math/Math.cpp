#include "Core/Math/Math.h"

namespace harpia {

Mat4 Mat4::translation(Vec3 offset) noexcept
{
    Mat4 result;
    result.at(0, 3) = offset.x;
    result.at(1, 3) = offset.y;
    result.at(2, 3) = offset.z;
    return result;
}

Mat4 Mat4::scale(Vec3 factors) noexcept
{
    Mat4 result;
    result.at(0, 0) = factors.x;
    result.at(1, 1) = factors.y;
    result.at(2, 2) = factors.z;
    return result;
}

Mat4 Mat4::rotationX(float radiansAngle) noexcept
{
    const float s = std::sin(radiansAngle);
    const float c = std::cos(radiansAngle);

    Mat4 result;
    result.at(1, 1) =  c;
    result.at(1, 2) = -s;
    result.at(2, 1) =  s;
    result.at(2, 2) =  c;
    return result;
}

Mat4 Mat4::rotationY(float radiansAngle) noexcept
{
    const float s = std::sin(radiansAngle);
    const float c = std::cos(radiansAngle);

    Mat4 result;
    result.at(0, 0) =  c;
    result.at(0, 2) =  s;
    result.at(2, 0) = -s;
    result.at(2, 2) =  c;
    return result;
}

Mat4 Mat4::rotationZ(float radiansAngle) noexcept
{
    const float s = std::sin(radiansAngle);
    const float c = std::cos(radiansAngle);

    Mat4 result;
    result.at(0, 0) =  c;
    result.at(0, 1) = -s;
    result.at(1, 0) =  s;
    result.at(1, 1) =  c;
    return result;
}

Mat4 Mat4::lookAt(Vec3 eye, Vec3 target, Vec3 up) noexcept
{
    // Right-handed: forward points from the target back towards the eye, so
    // the camera looks down -Z in view space.
    const Vec3 forward = normalize(eye - target);
    const Vec3 right   = normalize(cross(up, forward));
    const Vec3 trueUp  = cross(forward, right);

    Mat4 result;
    result.at(0, 0) = right.x;   result.at(0, 1) = right.y;   result.at(0, 2) = right.z;
    result.at(1, 0) = trueUp.x;  result.at(1, 1) = trueUp.y;  result.at(1, 2) = trueUp.z;
    result.at(2, 0) = forward.x; result.at(2, 1) = forward.y; result.at(2, 2) = forward.z;

    result.at(0, 3) = -dot(right, eye);
    result.at(1, 3) = -dot(trueUp, eye);
    result.at(2, 3) = -dot(forward, eye);
    return result;
}

Mat4 Mat4::perspectiveReverseZ(float fovYRadians, float aspect, float nearPlane) noexcept
{
    const float focal = 1.0f / std::tan(fovYRadians * 0.5f);

    Mat4 result;
    result.m[0]  = 0.0f; result.m[1]  = 0.0f; result.m[2]  = 0.0f; result.m[3]  = 0.0f;
    result.m[4]  = 0.0f; result.m[5]  = 0.0f; result.m[6]  = 0.0f; result.m[7]  = 0.0f;
    result.m[8]  = 0.0f; result.m[9]  = 0.0f; result.m[10] = 0.0f; result.m[11] = 0.0f;
    result.m[12] = 0.0f; result.m[13] = 0.0f; result.m[14] = 0.0f; result.m[15] = 0.0f;

    result.at(0, 0) = focal / aspect;
    // Negated: Vulkan clip space has +Y pointing down.
    result.at(1, 1) = -focal;
    // Infinite far with reverse-Z. Row 2 is zero except for near, so w = -z
    // and depth = near / -z: 1 at the near plane, approaching 0 at infinity.
    result.at(2, 3) = nearPlane;
    result.at(3, 2) = -1.0f;
    return result;
}

Mat4 Mat4::transposed() const noexcept
{
    Mat4 result;
    for (std::size_t row = 0; row < 4; ++row) {
        for (std::size_t column = 0; column < 4; ++column) {
            result.at(row, column) = at(column, row);
        }
    }
    return result;
}

Mat4 operator*(const Mat4& a, const Mat4& b) noexcept
{
    Mat4 result;
    for (std::size_t column = 0; column < 4; ++column) {
        for (std::size_t row = 0; row < 4; ++row) {
            float sum = 0.0f;
            for (std::size_t k = 0; k < 4; ++k) {
                sum += a.at(row, k) * b.at(k, column);
            }
            result.at(row, column) = sum;
        }
    }
    return result;
}

Vec4 operator*(const Mat4& m, const Vec4& v) noexcept
{
    return Vec4{
        m.at(0, 0) * v.x + m.at(0, 1) * v.y + m.at(0, 2) * v.z + m.at(0, 3) * v.w,
        m.at(1, 0) * v.x + m.at(1, 1) * v.y + m.at(1, 2) * v.z + m.at(1, 3) * v.w,
        m.at(2, 0) * v.x + m.at(2, 1) * v.y + m.at(2, 2) * v.z + m.at(2, 3) * v.w,
        m.at(3, 0) * v.x + m.at(3, 1) * v.y + m.at(3, 2) * v.z + m.at(3, 3) * v.w};
}

Vec3 transformPoint(const Mat4& m, Vec3 point) noexcept
{
    const Vec4 result = m * Vec4{point.x, point.y, point.z, 1.0f};
    return Vec3{result.x, result.y, result.z};
}

Vec3 transformDirection(const Mat4& m, Vec3 direction) noexcept
{
    const Vec4 result = m * Vec4{direction.x, direction.y, direction.z, 0.0f};
    return Vec3{result.x, result.y, result.z};
}

Vec2 encodeOctahedral(Vec3 normal) noexcept
{
    const Vec3 n = normalize(normal);
    const float sum = std::fabs(n.x) + std::fabs(n.y) + std::fabs(n.z);
    if (sum < 1e-8f) {
        return Vec2{0.0f, 0.0f};
    }

    Vec2 result{n.x / sum, n.y / sum};

    // Fold the lower hemisphere out onto the corners of the square.
    if (n.z < 0.0f) {
        const float x = result.x;
        const float y = result.y;
        result.x = (1.0f - std::fabs(y)) * (x >= 0.0f ? 1.0f : -1.0f);
        result.y = (1.0f - std::fabs(x)) * (y >= 0.0f ? 1.0f : -1.0f);
    }
    return result;
}

Vec3 decodeOctahedral(Vec2 encoded) noexcept
{
    Vec3 n{encoded.x, encoded.y, 1.0f - std::fabs(encoded.x) - std::fabs(encoded.y)};

    if (n.z < 0.0f) {
        const float x = n.x;
        const float y = n.y;
        n.x = (1.0f - std::fabs(y)) * (x >= 0.0f ? 1.0f : -1.0f);
        n.y = (1.0f - std::fabs(x)) * (y >= 0.0f ? 1.0f : -1.0f);
    }
    return normalize(n);
}

} // namespace harpia
