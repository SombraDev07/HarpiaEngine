// Harpia Engine — profiling macros
//
// Roadmap 1.9: instrumented from day one. When Tracy is disabled every macro
// compiles to nothing, so call sites never need guarding.
#pragma once

#if defined(HARPIA_TRACY_ENABLED)

#include <tracy/Tracy.hpp>

#define HARPIA_ZONE()                 ZoneScoped
#define HARPIA_ZONE_NAMED(name)       ZoneScopedN(name)
#define HARPIA_ZONE_COLORED(name, c)  ZoneScopedNC(name, c)
#define HARPIA_FRAME_MARK()           FrameMark
#define HARPIA_FRAME_MARK_NAMED(name) FrameMarkNamed(name)
#define HARPIA_THREAD_NAME(name)      tracy::SetThreadName(name)
#define HARPIA_PLOT(name, value)      TracyPlot(name, value)
#define HARPIA_MESSAGE(text)          TracyMessageL(text)

#else

#define HARPIA_ZONE()                 do {} while (false)
#define HARPIA_ZONE_NAMED(name)       do { (void)sizeof(name); } while (false)
#define HARPIA_ZONE_COLORED(name, c)  do { (void)sizeof(name); (void)(c); } while (false)
#define HARPIA_FRAME_MARK()           do {} while (false)
#define HARPIA_FRAME_MARK_NAMED(name) do { (void)sizeof(name); } while (false)
#define HARPIA_THREAD_NAME(name)      do { (void)sizeof(name); } while (false)
#define HARPIA_PLOT(name, value)      do { (void)sizeof(name); (void)(value); } while (false)
#define HARPIA_MESSAGE(text)          do { (void)sizeof(text); } while (false)

#endif
