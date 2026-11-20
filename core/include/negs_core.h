/*
 * negs_core.h: the C ABI of Negs' shared app core (core/, written in Odin).
 *
 * One flat C99 header for Swift (CNegsCore), Rust (windows/, linux/) and C.
 * Plan and rules: docs/shared-core-plan.md.
 *
 * Conventions
 * - Integers have explicit widths; booleans are uint8_t (0 or 1).
 * - Strings going in are UTF-8 as (pointer, length) and need not be
 *   NUL-terminated; a NULL pointer with length 0 is the empty string.
 * - Static strings coming out (theme names, the version) are NUL-terminated
 *   and live as long as the process.
 * - Variable-length results the caller owns come back in one block and are
 *   freed with the matching *_free call (negs_buf_free, negs_strokes_free).
 *   Nothing the core returns points into memory it will reuse unless the
 *   function says so.
 * - Every function is safe to call from any thread unless it takes a
 *   handle; a handle is used from one thread at a time.
 */
#ifndef NEGS_CORE_H
#define NEGS_CORE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Bumped on every change a host must be rebuilt for. */
#define NEGS_CORE_ABI_VERSION 2u

/* ------------------------------------------------------------------ */
/* Basics                                                              */
/* ------------------------------------------------------------------ */

/* An owned byte block: free it with negs_buf_free. */
typedef struct negs_buf_t {
    uint8_t *data;
    size_t len;
} negs_buf_t;

/* Memory the core allocated and hasn't freed, counted only in builds
 * made with `build.py lib --track` (tracked == 1); zeros otherwise. */
typedef struct negs_alloc_stats_t {
    int64_t live_bytes;
    int64_t live_count;
    int64_t total_count;
    int64_t bad_frees;
    uint8_t tracked;
} negs_alloc_stats_t;

/* NEGS_CORE_ABI_VERSION of the library linked in. */
uint32_t negs_core_abi_version(void);
/* "0.1.0 (odin dev-2026-11a)": static. */
const char *negs_core_version(void);
void negs_core_alloc_stats(negs_alloc_stats_t *out);

/* A copy of `len` bytes, for the hosts' smoke tests. */
negs_buf_t negs_buf_copy(const uint8_t *data, size_t len);
/* Frees a block from the core; a zero block is fine. */
void negs_buf_free(negs_buf_t buf);

/* Layout checks: the core's size_of / offset_of for "type" or
 * "type.field" (e.g. "negs_buf_t.len"); -1 when it doesn't know the name.
 * core/tests/c/layout_check.c compares them with the C compiler's. */
int64_t negs_core_layout(const char *name, size_t name_len);

/* ------------------------------------------------------------------ */
/* Marks: the pencil's geometry (core/src/marks; windows/marks)         */
/* ------------------------------------------------------------------ */
/* A photograph draws the same star, cross and box on every platform: the
 * same hash, random numbers and floating-point steps as the Swift and the
 * Rust they replace. Points are in the picture's coordinates, y down. */

typedef struct negs_pt_t {
    double x, y;
} negs_pt_t;

typedef struct negs_rect_t {
    double x, y, w, h;
} negs_rect_t;

#define NEGS_SEG_MOVE 0u
#define NEGS_SEG_LINE 1u
#define NEGS_SEG_CUBIC 2u
#define NEGS_SEG_CLOSE 3u

/* One step of a path. Move and Line use p[0]; Cubic is p[0] and p[1] (the
 * control points), then p[2] (the end); Close uses none. */
typedef struct negs_seg_t {
    uint32_t kind;
    uint32_t reserved;
    negs_pt_t p[3];
} negs_seg_t;

/* A ring's corner radii, CSS border-radius style: horizontal and vertical
 * per corner, clockwise from the top left; relative: shares of the width
 * and height rather than points. */
typedef struct negs_radii_t {
    double horizontal[4];
    double vertical[4];
    uint8_t relative;
} negs_radii_t;

#define NEGS_RADII_BOX 0u
#define NEGS_RADII_NUMBER 1u

#define NEGS_CROSS_LOOSE 1u
#define NEGS_CROSS_TWICE 2u

#define NEGS_BOX_FOUR 1u
#define NEGS_BOX_LOOP 2u
#define NEGS_BOX_TWO 3u

/* Filled outlines, in one block: stroke i is counts[i] points of `points`,
 * after the strokes before it, drawn at opacities[i] (1 unless a cross's).
 * kind: NEGS_CROSS_* or NEGS_BOX_* (0 for a star); tilt: a box's turn round
 * the picture's centre in degrees, clockwise. Free with negs_strokes_free. */
typedef struct negs_strokes_t {
    negs_pt_t *points;
    uint32_t *counts;
    double *opacities;
    size_t stroke_count;
    size_t point_count;
    uint32_t kind;
    uint32_t reserved;
    double tilt;
} negs_strokes_t;

typedef struct negs_marks_constants_t {
    double pencil_width;      /* the stroke under a word, not scaled */
    double pencil_height;     /* the box it's stretched to */
    double highlighter_width; /* Light table's band, less its round ends */
    double ring_line_width;
    double ring_outset_w;     /* how far a ring sits outside its picture */
    double ring_outset_h;
    double star_size;         /* the box each star is drawn in */
    double star_gap;
    double reach;             /* how far outside its picture a box may draw */
} negs_marks_constants_t;

void negs_marks_constants(negs_marks_constants_t *out);
void negs_marks_radii(uint32_t which, negs_radii_t *out);

/* Paths: write up to `cap` segments into `out` (NULL to ask) and return
 * how many the path has: the pencil at most 3, a ring 10. */
size_t negs_marks_pencil(negs_rect_t rect, uint8_t plain, negs_seg_t *out, size_t cap);
/* radii NULL: NEGS_RADII_BOX. */
size_t negs_marks_ring(negs_rect_t rect, const negs_radii_t *radii, uint8_t plain, negs_seg_t *out, size_t cap);
double negs_marks_ring_tilt(double degrees, uint8_t plain);
/* The box round the path itself (Bezier extremes, not control points):
 * 0 when there are no points. */
uint8_t negs_marks_path_bounds(const negs_seg_t *segs, size_t count, negs_rect_t *out);

/* FNV-1a over the name's UTF-16 units, and the next mulberry32 number in
 * [0, 1) from *state, as the mockup pages' scripts. */
uint32_t negs_marks_hash(const char *name, size_t len);
double negs_marks_random_next(uint32_t *state);

uint32_t negs_marks_star_seed(const char *name, size_t len, uint32_t index);
/* The index-th star in the name's row: three strokes round (0, 0). */
negs_strokes_t negs_marks_star(const char *name, size_t len, uint32_t index);

uint32_t negs_marks_cross_kind(const char *name, size_t len);
/* A reject's cross over a w x h picture; no strokes when either is <= 0. */
negs_strokes_t negs_marks_cross(double w, double h, const char *name, size_t len);

uint32_t negs_marks_box_kind(const char *name, size_t len);
/* A pick's box `outset` outside a w x h picture, the name used as given. */
negs_strokes_t negs_marks_pick_box(double w, double h, double outset, const char *name, size_t len,
                                   uint8_t tight, double weight);
/* The grid's (3 pt out), the strip's (2 pt, tight) and the selection's
 * (8 pt, the name + "\xC2\xB7second", weight 4 / 2.6). */
negs_strokes_t negs_marks_pick_box_grid(double w, double h, const char *name, size_t len);
negs_strokes_t negs_marks_pick_box_strip(double w, double h, const char *name, size_t len);
negs_strokes_t negs_marks_pick_box_selection(double w, double h, const char *name, size_t len);

/* Frees the block and zeroes *strokes; a zeroed one is fine. */
void negs_strokes_free(negs_strokes_t *strokes);

#ifdef __cplusplus
}
#endif

#endif /* NEGS_CORE_H */
