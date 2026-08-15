// Single translation unit for header-only third-party implementations.
//
// Their warnings are not ours to fix, so this file is deliberately excluded
// from harpia_warnings in CMakeLists.txt.

#define VMA_IMPLEMENTATION
#include <vk_mem_alloc.h>

#define STB_IMAGE_WRITE_IMPLEMENTATION
#include <stb_image_write.h>
