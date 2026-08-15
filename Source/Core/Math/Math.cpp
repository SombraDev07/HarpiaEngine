#include "Core/Math/Math.h"

#include <cmath>

namespace harpia {

Mat4 perspectiveReverseZ(float fovYRadians, float aspect, float nearPlane) noexcept
{
    const float focal = 1.0f / std::tan(fovYRadians * 0.5f);

    // Built by hand rather than patched from glm::perspective: with an infinite
    // far plane and swapped depth, almost every element differs, so deriving it
    // from glm's would be less clear, not more.
    Mat4 result(0.0f);
    result[0][0] = focal / aspect;
    result[1][1] = -focal;      // Vulkan clip space has +Y down
    result[2][3] = -1.0f;       // w = -z
    result[3][2] = nearPlane;   // depth = near / -z: 1 at near, 0 at infinity
    return result;
}

Mat4 orthographicReverseZ(float left, float right,
                          float bottom, float top,
                          float nearPlane, float farPlane) noexcept
{
    Mat4 result(1.0f);
    result[0][0] =  2.0f / (right - left);
    result[1][1] = -2.0f / (top - bottom);   // Y down
    // depth = (z + far) / (far - near): 1 at z = -near, 0 at z = -far. Writing
    // it the other way round yields ordinary Z, which the GREATER_OR_EQUAL
    // depth test would then read backwards.
    result[2][2] =  1.0f / (farPlane - nearPlane);
    result[3][0] = -(right + left) / (right - left);
    result[3][1] =  (top + bottom) / (top - bottom);
    result[3][2] =  farPlane / (farPlane - nearPlane);
    return result;
}

Mat3 normalMatrix(const Mat4& model) noexcept
{
    return glm::inverseTranspose(Mat3(model));
}

Vec2 encodeOctahedral(Vec3 normal) noexcept
{
    const float lengthSquared = glm::dot(normal, normal);
    if (lengthSquared < 1e-16f) {
        return Vec2(0.0f);
    }
    const Vec3 n = normal / std::sqrt(lengthSquared);

    const float sum = std::fabs(n.x) + std::fabs(n.y) + std::fabs(n.z);
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

    const float lengthSquared = glm::dot(n, n);
    return lengthSquared > 1e-16f ? n / std::sqrt(lengthSquared) : Vec3(0.0f);
}

// --- Transform --------------------------------------------------------------

Mat4 Transform::matrix() const noexcept
{
    Mat4 result = glm::mat4_cast(rotation);
    result[0] *= scale.x;
    result[1] *= scale.y;
    result[2] *= scale.z;
    result[3] = Vec4(position, 1.0f);
    return result;
}

Vec3 Transform::forward() const noexcept
{
    // Right-handed: the camera and objects look down their own -Z.
    return rotation * Vec3(0.0f, 0.0f, -1.0f);
}

Vec3 Transform::right() const noexcept
{
    return rotation * Vec3(1.0f, 0.0f, 0.0f);
}

Vec3 Transform::up() const noexcept
{
    return rotation * Vec3(0.0f, 1.0f, 0.0f);
}

// --- AABB -------------------------------------------------------------------

void AABB::grow(Vec3 point) noexcept
{
    min = glm::min(min, point);
    max = glm::max(max, point);
}

void AABB::grow(const AABB& other) noexcept
{
    if (!other.valid()) {
        return;
    }
    grow(other.min);
    grow(other.max);
}

Vec3 AABB::centre() const noexcept
{
    return (min + max) * 0.5f;
}

Vec3 AABB::extent() const noexcept
{
    return (max - min) * 0.5f;
}

bool AABB::valid() const noexcept
{
    return min.x <= max.x && min.y <= max.y && min.z <= max.z;
}

bool AABB::contains(Vec3 point) const noexcept
{
    return point.x >= min.x && point.x <= max.x
        && point.y >= min.y && point.y <= max.y
        && point.z >= min.z && point.z <= max.z;
}

AABB AABB::transformed(const Mat4& matrix) const noexcept
{
    if (!valid()) {
        return *this;
    }

    // Transform the centre, then grow the extent by the absolute matrix. Eight
    // corner transforms give the same box for an affine matrix at three times
    // the cost.
    const Vec3 oldCentre = centre();
    const Vec3 oldExtent = extent();

    const Vec3 newCentre = Vec3(matrix * Vec4(oldCentre, 1.0f));

    const Mat3 absolute{
        glm::abs(Vec3(matrix[0])),
        glm::abs(Vec3(matrix[1])),
        glm::abs(Vec3(matrix[2]))};
    const Vec3 newExtent = absolute * oldExtent;

    AABB result;
    result.min = newCentre - newExtent;
    result.max = newCentre + newExtent;
    return result;
}

// --- Frustum ----------------------------------------------------------------

float Plane::signedDistance(Vec3 point) const noexcept
{
    return glm::dot(normal, point) + distance;
}

Frustum Frustum::fromViewProjection(const Mat4& viewProjection) noexcept
{
    // Gribb-Hartmann: each plane is a sum or difference of matrix rows. glm is
    // column-major, so a "row" here is a component across the four columns.
    const Vec4 row0{viewProjection[0][0], viewProjection[1][0],
                    viewProjection[2][0], viewProjection[3][0]};
    const Vec4 row1{viewProjection[0][1], viewProjection[1][1],
                    viewProjection[2][1], viewProjection[3][1]};
    const Vec4 row2{viewProjection[0][2], viewProjection[1][2],
                    viewProjection[2][2], viewProjection[3][2]};
    const Vec4 row3{viewProjection[0][3], viewProjection[1][3],
                    viewProjection[2][3], viewProjection[3][3]};

    const auto makePlane = [](const Vec4& coefficients) {
        Plane plane;
        const Vec3 normal{coefficients.x, coefficients.y, coefficients.z};
        const float length = glm::length(normal);
        if (length > 1e-8f) {
            plane.normal   = normal / length;
            plane.distance = coefficients.w / length;
        }
        return plane;
    };

    Frustum frustum;
    frustum.planes_[Left]   = makePlane(row3 + row0);
    frustum.planes_[Right]  = makePlane(row3 - row0);
    frustum.planes_[Bottom] = makePlane(row3 + row1);
    frustum.planes_[Top]    = makePlane(row3 - row1);
    // Reverse-Z with depth in [0,1]: near is the w - z plane and far is z.
    frustum.planes_[Near]   = makePlane(row3 - row2);
    frustum.planes_[Far]    = makePlane(row2);
    return frustum;
}

bool Frustum::intersects(const AABB& box) const noexcept
{
    if (!box.valid()) {
        return false;
    }

    const Vec3 centre = box.centre();
    const Vec3 extent = box.extent();

    for (const Plane& plane : planes_) {
        // Project the extent onto the plane normal: the box is outside only if
        // its nearest corner is still behind the plane.
        const float radius = glm::dot(extent, glm::abs(plane.normal));
        if (plane.signedDistance(centre) < -radius) {
            return false;
        }
    }
    return true;
}

bool Frustum::contains(Vec3 point) const noexcept
{
    for (const Plane& plane : planes_) {
        if (plane.signedDistance(point) < 0.0f) {
            return false;
        }
    }
    return true;
}

} // namespace harpia
