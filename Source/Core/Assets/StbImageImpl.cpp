// stb_image implementation. Excluded from harpia_warnings in CMakeLists.txt —
// its warnings are not ours. stb_image_write lives in the RHI's own impl TU;
// the two define different symbols and do not collide.

#define STB_IMAGE_IMPLEMENTATION
#include <stb_image.h>
