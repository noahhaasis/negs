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
    SIZE(negs_pt_t);
    FIELD(negs_pt_t, y);
    SIZE(negs_rect_t);
    FIELD(negs_rect_t, h);
    SIZE(negs_seg_t);
    FIELD(negs_seg_t, kind);
    FIELD(negs_seg_t, p);
    SIZE(negs_radii_t);
    FIELD(negs_radii_t, vertical);
    FIELD(negs_radii_t, relative);
    SIZE(negs_strokes_t);
    FIELD(negs_strokes_t, points);
    FIELD(negs_strokes_t, counts);
    FIELD(negs_strokes_t, opacities);
    FIELD(negs_strokes_t, stroke_count);
    FIELD(negs_strokes_t, point_count);
    FIELD(negs_strokes_t, kind);
    FIELD(negs_strokes_t, tilt);
    SIZE(negs_marks_constants_t);
    FIELD(negs_marks_constants_t, reach);
    SIZE(negs_theme_hex_t);
    FIELD(negs_theme_hex_t, on_fill);
    FIELD(negs_theme_hex_t, blank);
    SIZE(negs_theme_t);
    FIELD(negs_theme_t, film);
    FIELD(negs_theme_t, dark);
    FIELD(negs_theme_t, plain);
    FIELD(negs_theme_t, id);
    FIELD(negs_theme_t, key);
    FIELD(negs_theme_t, title);
    SIZE(negs_theme_constants_t);
    FIELD(negs_theme_constants_t, highlighter_opacity);
    FIELD(negs_theme_constants_t, dimming_light);

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
    negs_strokes_t star = negs_marks_star("DSC_6435.NEF", 12, 0);
    if (star.stroke_count != 3 || star.point_count != 84) {
        fprintf(stderr, "negs_marks_star: %zu strokes, %zu points\n", star.stroke_count, star.point_count);
        failures++;
    }
    negs_strokes_free(&star);
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
