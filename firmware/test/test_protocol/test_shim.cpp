// SPDX-License-Identifier: AGPL-3.0-or-later
// Host test runner glue for the non-Unity build (firmware/ci/host_tests.sh).
#include "test_shim.h"

int omTestsRun = 0;
int omTestsFailed = 0;
int omAssertions = 0;
int omTestCurrentFailed = 0;

void omTestFail(const char* file, int line, const char* expr) {
    ++omTestsFailed;
    if (!omTestCurrentFailed) {
        std::printf("  FAIL %s:%d: %s\n", file, line, expr);
        omTestCurrentFailed = 1;
    }
}

void omRunTest(const char* name, void (*fn)()) {
    ++omTestsRun;
    omTestCurrentFailed = 0;
    std::printf("[ RUN  ] %s\n", name);
    fn();
    if (omTestCurrentFailed) {
        std::printf("[ FAIL ] %s\n", name);
    } else {
        std::printf("[  OK  ] %s\n", name);
    }
}
