#define DOCTEST_CONFIG_IMPLEMENT
#include <doctest/doctest.h>

#include "Core/Threading/JobSystem.h"

int main(int argc, char** argv)
{
    doctest::Context context;
    context.applyCommandLine(argc, argv);

    // Workers are up for the whole run; individual tests do not restart them.
    harpia::JobSystem::get().init();

    const int result = context.run();

    harpia::JobSystem::get().shutdown();

    return result;
}
