// SPDX-License-Identifier: AGPL-3.0-or-later
// Tiny Unity-compatible assertion shim so the same test_protocol.cpp runs both under
// `pio test -e native` (PlatformIO Unity) and the local host runner (firmware/ci/host_tests.sh)
// built with zig/gcc only.
#ifndef OPENMESH_TEST_SHIM_H
#define OPENMESH_TEST_SHIM_H

#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>

extern int omTestsRun;
extern int omTestsFailed;
extern int omAssertions;
extern int omTestCurrentFailed;

void omTestFail(const char* file, int line, const char* expr);
void omRunTest(const char* name, void (*fn)());

#define OM_TEST_FAIL(expr) omTestFail(__FILE__, __LINE__, expr)

#define TEST_ASSERT_TRUE(cond)                        \
    do {                                              \
        ++omAssertions;                               \
        if (!(cond)) {                                \
            OM_TEST_FAIL(#cond);                      \
        }                                             \
    } while (0)

#define TEST_ASSERT_FALSE(cond)                       \
    do {                                              \
        ++omAssertions;                               \
        if ((cond)) {                                 \
            OM_TEST_FAIL("expected false: " #cond);   \
        }                                             \
    } while (0)

#define TEST_ASSERT_NULL(ptr)                         \
    do {                                              \
        ++omAssertions;                               \
        if ((ptr) != nullptr) {                       \
            OM_TEST_FAIL("expected null: " #ptr);     \
        }                                             \
    } while (0)

#define TEST_ASSERT_NOT_NULL(ptr)                     \
    do {                                              \
        ++omAssertions;                               \
        if ((ptr) == nullptr) {                       \
            OM_TEST_FAIL("expected non-null: " #ptr); \
        }                                             \
    } while (0)

#define OM_TEST_CMP_EQ(a, b, fmt)                                                       \
    do {                                                                                \
        ++omAssertions;                                                                 \
        const auto om_a = (a);                                                          \
        const auto om_b = (b);                                                          \
        if (!(om_a == om_b)) {                                                          \
            char om_msg[160];                                                           \
            std::snprintf(om_msg, sizeof(om_msg), "expected " fmt " == " fmt, om_a, om_b); \
            OM_TEST_FAIL(om_msg);                                                       \
        }                                                                               \
    } while (0)

#define TEST_ASSERT_EQUAL_INT(a, b) OM_TEST_CMP_EQ(a, b, "%d")
#define TEST_ASSERT_EQUAL_UINT(a, b) OM_TEST_CMP_EQ(a, b, "%u")
#define TEST_ASSERT_EQUAL_UINT8(a, b) OM_TEST_CMP_EQ((unsigned)(a), (unsigned)(b), "%u")
#define TEST_ASSERT_EQUAL_UINT16(a, b) OM_TEST_CMP_EQ((unsigned)(a), (unsigned)(b), "%u")
#define TEST_ASSERT_EQUAL_UINT32(a, b) OM_TEST_CMP_EQ((unsigned long)(a), (unsigned long)(b), "%lu")
#define TEST_ASSERT_EQUAL_UINT64(a, b)                                    \
    OM_TEST_CMP_EQ((unsigned long long)(a), (unsigned long long)(b), "%llu")
#define TEST_ASSERT_EQUAL_HEX8(a, b) OM_TEST_CMP_EQ((unsigned)(a), (unsigned)(b), "0x%x")
#define TEST_ASSERT_EQUAL_HEX16(a, b) OM_TEST_CMP_EQ((unsigned)(a), (unsigned)(b), "0x%x")
#define TEST_ASSERT_EQUAL_HEX32(a, b) OM_TEST_CMP_EQ((unsigned long)(a), (unsigned long)(b), "0x%lx")

#define TEST_ASSERT_EQUAL_STRING(a, b)                                          \
    do {                                                                        \
        ++omAssertions;                                                         \
        const std::string om_a((a) == nullptr ? "" : (const char*)(a));        \
        const std::string om_b((b) == nullptr ? "" : (const char*)(b));        \
        if (om_a != om_b) {                                                     \
            OM_TEST_FAIL("strings differ: " #a " vs " #b);                      \
        }                                                                       \
    } while (0)

#define TEST_ASSERT_EQUAL_MEMORY(a, b, len)                                       \
    do {                                                                          \
        ++omAssertions;                                                           \
        if (std::memcmp((a), (b), (len)) != 0) {                                  \
            OM_TEST_FAIL("memory differs: " #a " vs " #b);                        \
        }                                                                         \
    } while (0)

#define RUN_TEST(fn) omRunTest(#fn, fn)

#endif  // OPENMESH_TEST_SHIM_H
