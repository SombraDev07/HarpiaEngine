// Harpia Engine — procedural meshes
//
// A sphere grid is how PBR gets validated visually: roughness across one axis,
// metallic across the other, one light. Anything wrong in the BRDF, the normal
// encoding or the tonemap shows up as a row that does not progress smoothly.
//
// Procedural rather than an imported asset on purpose — it needs no download,
// no textures and no art pipeline, so it stays runnable from a clean checkout.
#pragma once

#include "Core/Assets/MeshAsset.h"

#include <cstdint>

namespace harpia::rhi {

// UV sphere. `segments` is the longitude count, `rings` the latitude count.
[[nodiscard]] MeshAsset makeSphere(float         radius   = 1.0f,
                                   std::uint32_t segments = 32,
                                   std::uint32_t rings    = 16);

// Unit cube with flat shading — six materials' worth of distinct normals.
[[nodiscard]] MeshAsset makeCube(float size = 1.0f);

// Ground plane on XZ, facing +Y.
[[nodiscard]] MeshAsset makePlane(float size = 10.0f);

} // namespace harpia::rhi
