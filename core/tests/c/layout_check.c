/* Holds negs_core.h's structs to the core's own layout: every sizeof and
 * offsetof here must equal what negs_core_layout reports. Built and run by
 * `python3 core/build.py test`. */
#include <stdio.h>
#include <string.h>
#include "negs_core.h"

static int failures = 0;

static void check(const char *name, long long c_value) {
    long long core_value = (long long)negs_core_layout(name, strlen(name));
    if (core_value != c_value) {
        fprintf(stderr, "layout: %s is %lld in C, %lld in the core\n", name, c_value, core_value);
        failures++;
    }
}

#define SIZE(T) check(#T, (long long)sizeof(T))
#define FIELD(T, f) check(#T "." #f, (long long)offsetof(T, f))

int main(void) {
    SIZE(negs_buf_t);
    FIELD(negs_buf_t, data);
    FIELD(negs_buf_t, len);
    SIZE(negs_alloc_stats_t);
    FIELD(negs_alloc_stats_t, live_bytes);
    FIELD(negs_alloc_stats_t, live_count);
    FIELD(negs_alloc_stats_t, total_count);
    FIELD(negs_alloc_stats_t, bad_frees);
    FIELD(negs_alloc_stats_t, tracked);

    /* The smoke test: the version, and a block out and back. */
    if (negs_core_abi_version() != NEGS_CORE_ABI_VERSION) {
        fprintf(stderr, "abi version %u, header %u\n", negs_core_abi_version(), NEGS_CORE_ABI_VERSION);
        failures++;
    }
    const uint8_t bytes[3] = {7, 8, 9};
    negs_buf_t buf = negs_buf_copy(bytes, 3);
    if (buf.len != 3 || memcmp(buf.data, bytes, 3) != 0) {
        fprintf(stderr, "negs_buf_copy gave the wrong bytes\n");
        failures++;
    }
    negs_buf_free(buf);
    negs_alloc_stats_t stats;
    negs_core_alloc_stats(&stats);
    if (stats.tracked && (stats.live_count != 0 || stats.bad_frees != 0)) {
        fprintf(stderr, "leaks across the ABI: %lld live, %lld bad frees\n",
                (long long)stats.live_count, (long long)stats.bad_frees);
        failures++;
    }
    if (failures == 0) {
        printf("layout_check: ok (%s, tracked %d, %lld allocations)\n", negs_core_version(), stats.tracked,
               (long long)stats.total_count);
    }
    return failures == 0 ? 0 : 1;
}
