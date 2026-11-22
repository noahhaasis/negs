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
#define NEGS_CORE_ABI_VERSION 4u

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

/* ------------------------------------------------------------------ */
/* Theme: the built-in themes' tokens (core/src/theme)                  */
/* ------------------------------------------------------------------ */
/* Ids are the order themes are offered in; keys are the stored names (the
 * macOS app's AppTheme raw values). Colours are 0xRRGGBB, sRGB. */

#define NEGS_THEME_DARKROOM 0u
#define NEGS_THEME_CONTACT_SHEET 1u
#define NEGS_THEME_LIGHT_TABLE 2u
#define NEGS_THEME_DAYLIGHT 3u
#define NEGS_THEME_PLAIN 4u
#define NEGS_THEME_PLAIN_DARK 5u

#define NEGS_PLATFORM_MACOS 0u
#define NEGS_PLATFORM_WINDOWS 1u
#define NEGS_PLATFORM_LINUX 2u

typedef struct negs_theme_hex_t {
    uint32_t bg;      /* the sheet the photograph lies on, the title band */
    uint32_t panel;   /* the inspector, Settings, the alert */
    uint32_t film;    /* the strip, the tray, the leader */
    uint32_t grid;    /* behind the grid */
    uint32_t line;    /* rules between sections and modules */
    uint32_t track;   /* slider tracks, outlines, separators */
    uint32_t dim;     /* values at their default, hints */
    uint32_t label;   /* control labels */
    uint32_t paper;   /* what's being read */
    uint32_t accent;  /* the pencil's thin marks; never words */
    uint32_t fill;    /* the Export and Done buttons */
    uint32_t on_fill; /* the words on them */
    uint32_t warn;    /* errors and warnings, as words */
    uint32_t blank;   /* a tile not yet developed */
} negs_theme_hex_t;

typedef struct negs_theme_t {
    negs_theme_hex_t hex;
    negs_theme_hex_t film; /* the inks on the film: hex, but Contact sheet's */
    uint8_t dark;          /* light ink on dark: menus drawn dark */
    uint8_t highlighter;   /* the mark under a word is a highlighter */
    uint8_t plain;         /* the system font and upright marks */
    uint8_t reserved;
    uint32_t id;
    const char *key;       /* static */
    const char *title;     /* static */
} negs_theme_t;

typedef struct negs_theme_constants_t {
    uint32_t highlighter_ink;   /* Light table's highlighter */
    uint32_t crop_shade;        /* outside the crop box */
    double highlighter_opacity;
    double selection_opacity;   /* text selection: the pencil at this */
    double crop_shade_opacity;
    double disabled_opacity;
    double strip_dim;           /* strip frames other than the current one */
    double shadow_dark;         /* shadows under alerts and tags */
    double shadow_light;
    double dimming_dark[4];     /* under an alert: sRGB red, green, blue, opacity */
    double dimming_light[4];
} negs_theme_constants_t;

/* NEGS_THEME_* ids run 0 ..< negs_theme_count(). */
uint32_t negs_theme_count(void);
/* 0 for an unknown id. */
uint8_t negs_theme_get(uint32_t id, negs_theme_t *out);
/* The line under the swatches in Settings > Look; static. */
const char *negs_theme_hint(uint32_t id, uint32_t platform);
/* A stored key or a title, any case, spaces and punctuation ignored: its id,
 * or -1. */
int32_t negs_theme_parse(const char *text, size_t len);
/* WCAG 2: relative luminance 0..1, contrast ratio 1..21. */
double negs_theme_luminance(uint32_t rgb);
double negs_theme_contrast(uint32_t a, uint32_t b);
/* a moved towards b by t, each channel rounded half away from zero. */
uint32_t negs_theme_mix(uint32_t a, uint32_t b, double t);
void negs_theme_constants(negs_theme_constants_t *out);

/* ------------------------------------------------------------------ */
/* Library                                                             */
/* ------------------------------------------------------------------ */

/* A folder of photographs as a library (core/src/library; the Rust oracle
 * is crates/negs-library): its photographs and duplicates, their ratings,
 * flags and edits in the .negs sidecar beside each, the open photograph,
 * and undo. Every call works on the files at once.
 *
 * Compound values are compact UTF-8 JSON, in as (pointer, length) and out
 * in a negs_buf_t the caller frees. A result that can be absent is the
 * JSON `null`; an empty block means an argument wasn't understood. Paths
 * are absolute, as the folder's listing gives them. Shapes:
 *
 *   Item        {"photo": path, "variant": id}   (no "variant": the photograph)
 *   Summary     {"rating": 0-5, "flag": "pick"|"reject"|null,
 *                "edited": bool, "problem": text|null}
 *   EditState   {"film", "paper", "params": {...}, "crop": {...},
 *                "border": {...},
 *                "brightnessMatch": null | {"ev": number|null,
 *                                           "highlightLimited", "solved"},
 *                "roll": null | {"id", "magenta", "yellow", "paper": name|null}}
 *   EditDefaults {"params", "nullable": [path], "crop", "border",
 *                "films": {name: positive}, "papers": [name],
 *                "tableFilm", "tablePaper", "newPhotoFilm", "newPhotoPaper",
 *                "matchNewPhotos": bool}
 *   Filter      {"rating": {"kind": "any"|"atLeast"|"unrated", "stars": n},
 *                "flags": ["pick"|"unflagged"|"reject"],
 *                "edits": "any"|"edited"|"unedited"}
 *   Outcome     {"note": null | {"text", "detail": text|null, "count"},
 *                "error": text|null, "openChanged": bool, "openGone": bool,
 *                "open": Item|null, "select": [Item]}
 *   DeletePlan  {"items": [Item], "files": [path], "duplicates": [Item],
 *                "following": n, "companions": [[file, picture]],
 *                "renamed": [[from Item, to Item]]}
 *   Question    {"message", "information", "button"}
 *   PhotoFile   {"path", "name", "modified": [seconds, nanoseconds]|null,
 *                "size": bytes|null}
 *   Snapshot    {"folder", "files": [PhotoFile], "companions": [[raw, picture]],
 *                "duplicates": [[photo, [id]]], "summaries": [[Item, Summary]],
 *                "looks": [[Item, look]], "readOnly", "failed": text|null,
 *                "filter": Filter, "pairing", "sidecars", "openItem": Item|null,
 *                "undoName": text|null, "redoName": text|null,
 *                "undoDepth": [undo, redo]}
 *   Grid        [{"item": Item, "name", "summary": Summary, "look": text|null}]
 *   Groups      ["filmAndPaper"|"prep"|"lab"|"border"|"output"|
 *                "cropAndRotation"|"file"]
 */
typedef struct NegsLibrary NegsLibrary;

/* Where deleted photographs go: the platform's bin. Blocks the host
 * returns are made with negs_buf_copy; the core frees them. */
typedef struct negs_bin_t {
    void *user;
    /* What the platform calls it ("Trash"); NULL is "Trash". */
    negs_buf_t (*name)(void *user);
    /* Whether the folder's volume has a bin; without one a delete is
     * permanent. */
    uint8_t (*available)(void *user, const uint8_t *folder, size_t folder_len);
    /* 1: moved, *out is where it landed; 0: *out is why not. */
    uint8_t (*trash)(void *user, const uint8_t *file, size_t file_len, negs_buf_t *out);
    /* 1: put back; 0: *error is why not. */
    uint8_t (*put_back)(void *user, const uint8_t *trashed, size_t trashed_len,
                        const uint8_t *original, size_t original_len, negs_buf_t *error);
    /* Deletes at once; NULL unlinks the file. */
    uint8_t (*remove)(void *user, const uint8_t *file, size_t file_len, negs_buf_t *error);
    /* Called once when the library is freed (or isn't made). */
    void (*release)(void *user);
} negs_bin_t;

typedef struct negs_library_config_t {
    const uint8_t *folder;
    size_t folder_len;
    /* The engine's data folder, which holds sidecar-defaults/. */
    const uint8_t *data;
    size_t data_len;
    /* The app's version, written into each sidecar as "app". */
    const uint8_t *app;
    size_t app_len;
    negs_bin_t bin;
    /* Settings › Show camera JPEGs separately, off: 1. */
    uint8_t pairing;
    /* Settings › Save ratings and edits next to photos. */
    uint8_t sidecars;
    /* Seeds the ids of new variants; random when 0. */
    uint8_t has_id_seed;
    uint64_t id_seed;
} negs_library_config_t;

/* Change Each Edit's transform: an Item and an EditState in, the new
 * EditState out (made with negs_buf_copy). */
typedef negs_buf_t (*negs_transform_fn)(void *user, const uint8_t *item, size_t item_len,
                                        const uint8_t *state, size_t state_len);

/* The library of the folder, listed and its sidecars read; NULL and
 * *error (may be NULL) when the defaults tables can't be read. The bin is
 * the library's either way. */
NegsLibrary *negs_library_new(const negs_library_config_t *config, negs_buf_t *error);
void negs_library_free(NegsLibrary *library);

/* What it shows. `current` (an Item, or empty) is always shown. */
negs_buf_t negs_library_snapshot(NegsLibrary *library);                /* Snapshot */
negs_buf_t negs_library_items(NegsLibrary *library);                   /* [Item] */
negs_buf_t negs_library_shown(NegsLibrary *library, const uint8_t *current, size_t current_len);
negs_buf_t negs_library_grid(NegsLibrary *library, const uint8_t *current, size_t current_len);
negs_buf_t negs_library_rejected(NegsLibrary *library);                /* [Item] */
/* "IMG_0412.CR3 (2)": text, not JSON. */
negs_buf_t negs_library_name(NegsLibrary *library, const uint8_t *item, size_t item_len);
negs_buf_t negs_library_summary(NegsLibrary *library, const uint8_t *item, size_t item_len);
/* {"state": EditState, "matchSolved": bool} | null */
negs_buf_t negs_library_thumbnail_state(NegsLibrary *library, const uint8_t *item, size_t item_len);
/* {"state", "matchSolved", "missing": [text], "look"} | null */
negs_buf_t negs_library_export_edits(NegsLibrary *library, const uint8_t *item, size_t item_len);
uint8_t negs_library_store_solved_match(NegsLibrary *library, const uint8_t *item, size_t item_len,
                                        const uint8_t *look, size_t look_len,
                                        const uint8_t *state, size_t state_len);
negs_buf_t negs_library_unedited_state(NegsLibrary *library);          /* EditState */
negs_buf_t negs_library_defaults(NegsLibrary *library);                /* EditDefaults */
/* {"state": EditState, "groups": Groups, "edited": bool} | null */
negs_buf_t negs_library_copied(NegsLibrary *library);
negs_buf_t negs_library_previous_edits(NegsLibrary *library);          /* EditState | null */

/* Settings. */
negs_buf_t negs_library_set_defaults(NegsLibrary *library, const uint8_t *defaults, size_t defaults_len); /* Outcome */
uint8_t negs_library_set_filter(NegsLibrary *library, const uint8_t *filter, size_t filter_len);
void negs_library_set_pairing(NegsLibrary *library, uint8_t pairing);
void negs_library_set_sidecars(NegsLibrary *library, uint8_t on);
/* Lists the folder again: {"changed": [photo], "outcome": Outcome}. */
negs_buf_t negs_library_refresh(NegsLibrary *library, uint8_t interacting);

/* The open photograph. */
negs_buf_t negs_library_state(NegsLibrary *library);                   /* EditState | null */
uint8_t negs_library_open_edited(NegsLibrary *library);
uint8_t negs_library_unsaved(NegsLibrary *library);
/* {"state": EditState, "problem": text|null} */
negs_buf_t negs_library_open(NegsLibrary *library, const uint8_t *item, size_t item_len);
/* 1 saved (or nothing to save); 0 and *error (may be NULL) why not. */
uint8_t negs_library_close(NegsLibrary *library, negs_buf_t *error);
uint8_t negs_library_save(NegsLibrary *library, negs_buf_t *error);
uint8_t negs_library_edit(NegsLibrary *library, const uint8_t *state, size_t state_len);
negs_buf_t negs_library_opening_state(NegsLibrary *library, const uint8_t *item, size_t item_len); /* EditState | null */
/* A new variant or roll id: text. */
negs_buf_t negs_library_new_id(NegsLibrary *library);

/* Actions on items ([Item]), each one undo step; each returns an Outcome. */
negs_buf_t negs_library_set_rating(NegsLibrary *library, const uint8_t *items, size_t items_len, int64_t stars);
negs_buf_t negs_library_change_rating(NegsLibrary *library, const uint8_t *items, size_t items_len, int64_t by);
/* flag: 0 unflagged, 1 pick, 2 reject. */
negs_buf_t negs_library_set_flag(NegsLibrary *library, const uint8_t *items, size_t items_len, uint8_t flag);
negs_buf_t negs_library_copy_edits(NegsLibrary *library, const uint8_t *item, size_t item_len,
                                   const uint8_t *groups, size_t groups_len);
negs_buf_t negs_library_paste_edits(NegsLibrary *library, const uint8_t *items, size_t items_len);
negs_buf_t negs_library_paste_from_previous(NegsLibrary *library, const uint8_t *items, size_t items_len);
negs_buf_t negs_library_reset_edits(NegsLibrary *library, const uint8_t *items, size_t items_len);
negs_buf_t negs_library_change_each_edit(NegsLibrary *library, const uint8_t *items, size_t items_len,
                                         const uint8_t *action, size_t action_len,
                                         const uint8_t *note, size_t note_len,
                                         negs_transform_fn transform, void *user);
negs_buf_t negs_library_duplicate(NegsLibrary *library, const uint8_t *items, size_t items_len);

/* Deleting: the plan for [Item], the alert before it, and the delete. */
negs_buf_t negs_library_delete_plan(NegsLibrary *library, const uint8_t *items, size_t items_len);   /* DeletePlan */
negs_buf_t negs_library_delete_question(NegsLibrary *library, const uint8_t *plan, size_t plan_len); /* Question */
uint8_t negs_library_delete_is_permanent(NegsLibrary *library, const uint8_t *plan, size_t plan_len);
negs_buf_t negs_library_delete(NegsLibrary *library, const uint8_t *plan, size_t plan_len);          /* Outcome */

/* Undo and Redo: an Outcome, or null when there's nothing to undo. */
negs_buf_t negs_library_undo(NegsLibrary *library);
negs_buf_t negs_library_redo(NegsLibrary *library);

/* Pure helpers. */
/* A sidecar's bytes read and written again as the Mac writes them; 0 and
 * the reason in *out when they aren't JSON. */
uint8_t negs_sidecar_rewrite(const uint8_t *data, size_t len, negs_buf_t *out);
/* JSON compact with sorted keys and escaped slashes, as looks hash it. */
uint8_t negs_json_compact_sorted(const uint8_t *data, size_t len, negs_buf_t *out);
/* Thumbnail names: text. `variant` NULL for a photograph's own. */
negs_buf_t negs_thumbnail_look(const uint8_t *edits, size_t edits_len, int64_t defaults_version);
negs_buf_t negs_thumbnail_unedited_look(const uint8_t *state, size_t state_len);
negs_buf_t negs_thumbnail_key(const uint8_t *path, size_t path_len, uint8_t has_size, uint64_t size,
                              double modified, const uint8_t *variant, size_t variant_len,
                              const uint8_t *look, size_t look_len, const uint8_t *engine, size_t engine_len);
negs_buf_t negs_thumbnail_loose_key(const uint8_t *path, size_t path_len, uint8_t has_size, uint64_t size,
                                    double modified, const uint8_t *variant, size_t variant_len,
                                    const uint8_t *look, size_t look_len);
/* A PNG, JPEG or TIFF with xmp:Rating (stars, -1 reject); 0 for others. */
uint8_t negs_xmp_embed(const uint8_t *data, size_t len, int32_t rating, negs_buf_t *out);
/* 1 embedded, 0 not a PNG, JPEG or TIFF, 2 an error (*error). */
uint8_t negs_xmp_embed_in_file(const uint8_t *path, size_t path_len, int32_t rating, negs_buf_t *error);
/* The filter sheet's sentence: [{"lead", "word"}]. */
negs_buf_t negs_filter_words(const uint8_t *filter, size_t filter_len);
/* The grid foot's count: text. */
negs_buf_t negs_count_words(uint64_t shown, uint64_t total, uint64_t selected, uint8_t filtered);
/* Finder's order of two file names: -1, 0 or 1. */
int32_t negs_standard_compare(const uint8_t *a, size_t a_len, const uint8_t *b, size_t b_len);
uint8_t negs_can_write(const uint8_t *folder, size_t folder_len);
/* One change to a selection {"current", "selected", "anchor", "lead",
 * "base"} (Items; current/anchor/lead may be null): returns
 * {"selection", "items"}. The ops are listed in
 * core/src/abi/library_helpers.odin. */
negs_buf_t negs_selection_apply(const uint8_t *state, size_t state_len, const uint8_t *op, size_t op_len);

/* ------------------------------------------------------------------ */
/* Phase 3a: Sheet, tray and compare rules (core/src/rules: sheet, tray, compare) */
/* ------------------------------------------------------------------ */
/* (Phase 3a's declarations go here.) */





/* ------------------------------------------------------------------ */
/* Phase 3b: Picture rules: crop, border, exact fit, thumbnail compose, histogram, negative (core/src/rules) */
/* ------------------------------------------------------------------ */
/* The same rounding, clamping and floating-point steps as windows/src's
 * Rust (Rust's min/max ignore a NaN; float to integer casts saturate), so
 * results are bit for bit. Nothing here takes a handle or keeps state;
 * results go through out-pointers (NULL skips them) or come back in an
 * owned negs_buf_t. Booleans in structs are 0 or 1. Rectangles are
 * negs_rect_t (x, y, w, h) and points negs_pt_t, from Marks. */

typedef struct negs_size_t {
    double width, height;
} negs_size_t;

/* Crop (core/src/rules/crop; windows/src/crop.rs, Crop.swift).
 * A presentation crop: the rectangle is a fraction of the unrotated
 * photograph, origin top left; `angle` straightens it, degrees clockwise
 * as shown. */

#define NEGS_CROP_ORIGINAL 0u
#define NEGS_CROP_FREEFORM 1u
#define NEGS_CROP_SQUARE 2u
#define NEGS_CROP_16_9 3u
#define NEGS_CROP_3_2 4u
#define NEGS_CROP_4_3 5u
#define NEGS_CROP_5_4 6u
#define NEGS_CROP_7_5 7u
#define NEGS_CROP_CUSTOM 8u

typedef struct negs_crop_settings_t {
    negs_rect_t rect;
    negs_size_t custom; /* the custom ratio, width by height as shown */
    double angle;
    uint32_t aspect;    /* NEGS_CROP_* */
    uint32_t reserved;
} negs_crop_settings_t;

/* Mirrored left to right if flip, then `turns` quarter turns clockwise. */
typedef struct negs_orientation_t {
    int32_t turns;
    uint8_t flip;
    uint8_t reserved[3];
} negs_orientation_t;

/* An edge or corner: -1 left or top, 1 right or bottom; both 0 moves. */
typedef struct negs_crop_handle_t {
    int32_t x, y;
} negs_crop_handle_t;

/* The crop being drawn, its geometry (the host keeps the edit it began
 * from). Make it with negs_crop_session_new; the session calls change it
 * in place. `intended` (with has_intended) is the crop when straightening
 * began, as shown. */
typedef struct negs_crop_session_t {
    negs_rect_t rect;
    negs_size_t custom;
    negs_size_t photo;  /* the developed photograph as shown, in pixels */
    negs_rect_t intended;
    double angle;       /* as shown: the setting's, the other way round when flipped */
    int32_t rotation;
    uint32_t aspect;
    uint8_t portrait;
    uint8_t flip;
    uint8_t straightening;
    uint8_t has_intended;
    uint32_t reserved;
} negs_crop_session_t;

double negs_crop_max_angle(void);
/* Static strings; the format is NULL for a ratio without one. */
const char *negs_crop_aspect_title(uint32_t aspect);
const char *negs_crop_aspect_format(uint32_t aspect);
/* The long side over the short; 0 for freeform and custom. */
uint8_t negs_crop_aspect_ratio(uint32_t aspect, negs_size_t photo, double *out);
uint8_t negs_crop_aspect_turns(uint32_t aspect);
uint8_t negs_crop_cropped(const negs_crop_settings_t *crop);
/* The exporter's rounding (spektrafilm-io's Crop::pixels and
 * straightened_size): out[4] = x, y, width, height; out[2] = width, height. */
void negs_crop_engine_pixels(const negs_crop_settings_t *crop, uint32_t width, uint32_t height, uint32_t out[4]);
void negs_crop_straightened_size(const negs_crop_settings_t *crop, uint32_t width, uint32_t height, uint32_t out[2]);
void negs_crop_pixels(const negs_crop_settings_t *crop, negs_size_t size, negs_rect_t *out);
void negs_crop_size_of(const negs_crop_settings_t *crop, negs_size_t size, negs_size_t *out);
void negs_crop_shown(negs_rect_t rect, int32_t rotation, uint8_t flip, negs_rect_t *out);
void negs_crop_unrotated(negs_rect_t rect, int32_t rotation, uint8_t flip, negs_rect_t *out);
void negs_crop_mirrored(negs_rect_t rect, negs_rect_t *out);
void negs_rect_inset(negs_rect_t rect, double dx, double dy, negs_rect_t *out);
uint8_t negs_rect_contains(negs_rect_t rect, negs_pt_t p);
void negs_rect_intersection(negs_rect_t a, negs_rect_t b, negs_rect_t *out);
void negs_orientation_new(int32_t turns, uint8_t flip, negs_orientation_t *out);
void negs_orientation_flipped_as_shown(negs_orientation_t o, negs_orientation_t *out);
void negs_orientation_shown(negs_orientation_t o, negs_pt_t fraction, negs_pt_t *out);
void negs_orientation_unshown(negs_orientation_t o, negs_pt_t fraction, negs_pt_t *out);
/* A point shown through the crop back to a fraction of the photograph;
 * 0 outside it. */
uint8_t negs_crop_unshown_point(negs_orientation_t o, const negs_crop_settings_t *crop, negs_size_t turned, double x, double y,
                                negs_pt_t *out);
void negs_crop_fit(double ratio, negs_pt_t center, negs_rect_t bounds, negs_rect_t *out);
void negs_crop_move_rect(negs_rect_t rect, double dx, double dy, negs_rect_t bounds, negs_rect_t *out);
void negs_crop_resize(negs_rect_t rect, negs_crop_handle_t handle, negs_pt_t point, uint8_t has_ratio, double ratio,
                      negs_rect_t bounds, double minimum, negs_rect_t *out);
/* 0 when the point is out of reach. */
uint8_t negs_crop_handle_at(negs_pt_t point, negs_rect_t frame, double scale, negs_crop_handle_t *out);
void negs_crop_turn(negs_pt_t point, double degrees, negs_pt_t center, negs_pt_t *out);
void negs_crop_corners(negs_size_t photo, double angle, negs_pt_t out[4]);
void negs_crop_bounds(negs_size_t photo, double angle, negs_rect_t *out);
uint8_t negs_crop_fits(negs_rect_t rect, double angle, negs_size_t photo);
void negs_crop_constrain(negs_rect_t start, negs_rect_t target, double angle, negs_size_t photo, negs_rect_t *out);
void negs_crop_inscribe(negs_rect_t rect, double angle, negs_size_t photo, negs_rect_t *out);

void negs_crop_session_new(const negs_crop_settings_t *start, negs_size_t photo, int32_t rotation, uint8_t flip,
                           negs_crop_session_t *out);
void negs_crop_session_settings(const negs_crop_session_t *s, negs_crop_settings_t *out);
uint8_t negs_crop_session_modified(const negs_crop_session_t *s);
uint8_t negs_crop_session_orientation_portrait(const negs_crop_session_t *s);
/* Width over height of the locked ratio as shown; 0 for freeform. */
uint8_t negs_crop_session_ratio(const negs_crop_session_t *s, double *out);
void negs_crop_session_shown(const negs_crop_session_t *s, negs_rect_t *out);
void negs_crop_session_set_shown(negs_crop_session_t *s, negs_rect_t value);
void negs_crop_session_bounds(const negs_crop_session_t *s, negs_rect_t *out);
void negs_crop_session_update(negs_crop_session_t *s, negs_size_t photo, int32_t rotation, uint8_t flip);
void negs_crop_session_choose(negs_crop_session_t *s, uint32_t aspect);
void negs_crop_session_orient(negs_crop_session_t *s, uint8_t portrait);
void negs_crop_session_set_custom(negs_crop_session_t *s, uint8_t has_width, double width, uint8_t has_height, double height);
void negs_crop_session_reset(negs_crop_session_t *s);
void negs_crop_session_begin_straightening(negs_crop_session_t *s);
void negs_crop_session_straighten(negs_crop_session_t *s, double degrees);
void negs_crop_session_end_straightening(negs_crop_session_t *s);

/* Border (core/src/rules/border; windows/src/border.rs, Border.swift).
 * The engine's sizes (spektrafilm-io's border and film35 strip) are ported
 * and held to it bit for bit; the textures and drawing stay the engine's. */

#define NEGS_BORDER_SOLID 0u
#define NEGS_BORDER_FILM 1u
#define NEGS_BORDER_KEYLINE 2u
#define NEGS_BORDER_FILM35 3u

#define NEGS_FILM_FULL 0u
#define NEGS_FILM_HALF 1u

typedef struct negs_border_settings_t {
    double width;      /* the solid and keyline mat, a share of the long edge */
    double film_width; /* black around the film frame */
    double line;       /* the keyline's line */
    uint32_t style;    /* NEGS_BORDER_* */
    uint8_t color[3];  /* 8-bit sRGB */
    uint8_t line_color[3];
    uint8_t enabled;
    uint8_t export_border;
    uint8_t instagram;
    uint8_t reserved[3];
} negs_border_settings_t;

/* Static strings. */
const char *negs_border_style_title(uint32_t style);
const char *negs_border_style_help(uint32_t style);
const char *negs_border_style_raw_value(uint32_t style);
/* The styles in the inspector's order. */
void negs_border_styles(uint32_t out[4]);
uint8_t negs_border_fits(uint32_t style, uint32_t width, uint32_t height);
/* NEGS_FILM_*, or -1 when the photograph is neither 3:2 nor 3:4. */
int32_t negs_film_format_of(uint32_t width, uint32_t height);
const char *negs_film_format_name(uint32_t format);
uint32_t negs_border_margin(uint32_t width, uint32_t height, double fraction);
uint32_t negs_border_keyline_width(uint32_t width, uint32_t height, double fraction);
/* (inset, nine-patch size) */
void negs_border_film_geometry(uint32_t width, uint32_t height, uint32_t out[2]);
void negs_border_instagram_canvas(uint32_t width, uint32_t height, uint32_t style, uint32_t out[2]);
uint8_t negs_strip_turned(uint32_t format, uint32_t width, uint32_t height);
void negs_strip_canvas(uint32_t format, uint32_t width, uint32_t height, uint32_t out[2]);
/* Whether the 35mm strip turns to hold the photograph. */
uint8_t negs_border_turned(uint32_t width, uint32_t height);
/* "#rrggbb" and a NUL. */
void negs_border_color_hex(const uint8_t color[3], char out[8]);
/* "#rrggbb" or "rrggbb"; 0 when it isn't one. */
uint8_t negs_border_color_from_hex(const uint8_t *text, size_t len, uint8_t out[3]);
void negs_border_color_rgba(const uint8_t color[3], float out[4]);
uint8_t negs_border_shown(const negs_border_settings_t *border, uint32_t width, uint32_t height);
uint8_t negs_border_exported(const negs_border_settings_t *border, uint32_t width, uint32_t height);
/* The style's width setting; 0 for 35mm, which has none. */
uint8_t negs_border_style_width(const negs_border_settings_t *border, double *out);
uint32_t negs_border_keyline(const negs_border_settings_t *border, uint32_t width, uint32_t height);
/* The canvas around a width x height photograph, as the exporter makes it
 * (the 35mm strip's size doesn't depend on the film). */
void negs_border_canvas(const negs_border_settings_t *border, uint32_t width, uint32_t height, uint32_t out[2]);
/* Instagram's 3:4 canvas around a bordered width x height canvas. */
void negs_border_instagram(const negs_border_settings_t *border, uint32_t width, uint32_t height, uint32_t out[2]);
void negs_border_document(const negs_border_settings_t *border, uint32_t width, uint32_t height, uint32_t out[2]);
/* `spektrafilm process` arguments, each followed by a NUL. */
negs_buf_t negs_border_arguments(const negs_border_settings_t *border);
/* A double as Rust's `{}` writes it. */
negs_buf_t negs_rust_display_f64(double v);

/* Exact fit (core/src/rules/fit; windows/src/exact_fit.rs): Lanczos-3 in
 * linear light, as Pillow's. Pixels are the caller's. */

void negs_fit_target(double w, double h, double z, uint32_t out[2]);
uint8_t negs_fit_worth_it(double z, uint32_t texture_w, uint32_t texture_h, uint32_t target_w, uint32_t target_h);
/* Fills `output` starts and counts and the weights when weights_cap holds
 * them all (any NULL: fills nothing); returns how many weights there are. */
size_t negs_fit_coefficients(uint32_t input, uint32_t output, uint32_t *starts, uint32_t *counts, float *weights,
                             size_t weights_cap);
/* sRGB RGBA w x h into out (tw x th x 4 bytes, opaque), on up to 16
 * threads for the call. 0 when a size is 0 or a buffer too small. */
uint8_t negs_fit_resample(const uint8_t *rgba, size_t rgba_len, uint32_t w, uint32_t h, uint32_t tw, uint32_t th,
                          uint8_t *out, size_t out_len);

/* Thumbnail compose (core/src/rules/compose; windows/src/thumb_compose.rs):
 * the rules around the engine's crop and border, which the host calls. */

/* The size a preview is reduced to before a crop_w x crop_h (fractions)
 * crop is cut for a long_side thumbnail; 0 when it's small enough. */
uint8_t negs_compose_reduced_size(uint32_t width, uint32_t height, double crop_w, double crop_h, uint32_t long_side,
                                  uint32_t out[2]);
/* RGBA with Rec. 2020 primaries to sRGB's, in place; m is row-major. */
void negs_compose_to_srgb(uint8_t *rgba, size_t len, const float m[9]);

/* Histogram (core/src/rules/histogram; windows/src/histogram.rs). */

typedef struct negs_histogram_t {
    uint32_t red[256];
    uint32_t green[256];
    uint32_t blue[256];
} negs_histogram_t;

uint32_t negs_histogram_peak(const negs_histogram_t *h);
/* The waveform's width for a width x height preview; 0 when there is
 * nothing to count. */
size_t negs_scopes_wave_width(size_t width, size_t height, const negs_crop_settings_t *crop, negs_orientation_t o,
                              size_t max_samples);
/* Counts the crop of an RGBA preview into *h and the waveform's bins
 * (wave_len = wave width x 256 each, row-major, level 0 first); returns the
 * samples, 0 when there is nothing to count or wave_len doesn't match. */
size_t negs_scopes_count(const uint8_t *rgba, size_t rgba_len, size_t width, size_t height,
                         const negs_crop_settings_t *crop, negs_orientation_t o, size_t max_samples, negs_histogram_t *h,
                         uint32_t *red, uint32_t *green, uint32_t *blue, size_t wave_len);
/* The waveform drawn: width x 256 premultiplied RGBA pixels, level 255 on
 * top. 0 when a buffer is short. */
uint8_t negs_waveform_raster(size_t width, const uint32_t *red, const uint32_t *green, const uint32_t *blue, size_t len,
                             uint8_t *out, size_t out_pixels);

/* Negative (core/src/rules/negative; windows/src/negative.rs). The edit
 * state's fields come in as plain values. */

#define NEGS_NEGATIVE_ROUTE 0u
#define NEGS_NEGATIVE_SCENE_ROUTE 1u
#define NEGS_NEGATIVE_ROUTE_PATH 2u
#define NEGS_NEGATIVE_DEFAULT_FILM 3u
#define NEGS_NEGATIVE_STOCK_ID 4u
#define NEGS_NEGATIVE_NAME 5u
#define NEGS_NEGATIVE_TILE_TEXT 6u
#define NEGS_NEGATIVE_SCAN_ILLUMINANT 7u
#define NEGS_NEGATIVE_FLAT_FIELD 8u
#define NEGS_NEGATIVE_FLAT_FIELD_HELP 9u

#define NEGS_NEGATIVE_OWN_PATHS 0u
#define NEGS_NEGATIVE_FOUND_PATHS 1u
#define NEGS_NEGATIVE_PRESET_KEEPS 2u
#define NEGS_NEGATIVE_SCAN_READING 3u

#define NEGS_PICK_BASE 0u
#define NEGS_PICK_NEUTRAL 1u

/* What a camera recorded of a scan's light: shutter (s), aperture, ISO. */
typedef struct negs_scan_light_t {
    double value[3];
    uint8_t has[3];
    uint8_t reserved[5];
} negs_scan_light_t;

/* Static strings; NULL for an unknown name or past a list's end. */
const char *negs_negative_text(uint32_t which);
const char *negs_negative_path(uint32_t list, size_t index);
/* 0 when the two share nothing recorded. */
uint8_t negs_negative_scan_stops(negs_scan_light_t frame, negs_scan_light_t reference, double *out);
double negs_negative_exposure_ev(double roll, uint8_t has_stops, double stops);
negs_buf_t negs_negative_skipped(uint64_t count);
uint8_t negs_negative_is_route(const uint8_t *route, size_t len);
uint8_t negs_negative_readable(uint8_t paper, uint8_t positive, uint8_t bw);
uint8_t negs_negative_is_found(const uint8_t *path, size_t len);
/* The crop to the picture the rebate finder found (NaN for a missing
 * corner); 0 unless it lies in the photograph. */
uint8_t negs_negative_picture_crop(double x0, double y0, double x1, double y1, double angle, negs_crop_settings_t *out);
negs_buf_t negs_flat_field_value(const uint8_t *path, size_t len);
/* used: -1 not said yet, 0 not used, 1 used. 0 when there's no hint. */
uint8_t negs_flat_field_hint(const uint8_t *path, size_t len, int32_t used, uint8_t has_reason, const uint8_t *reason,
                             size_t reason_len, negs_buf_t *out);
const char *negs_negative_not_negative(uint8_t raw);
/* The edge as shown (static); NULL for a word that isn't an edge. */
const char *negs_negative_shown_edge(negs_orientation_t o, const uint8_t *edge, size_t len);
uint8_t negs_base_status_guessed(uint8_t found, uint8_t picked, uint8_t by_hand, uint8_t from_roll);
negs_buf_t negs_base_status_sentence(uint8_t found, uint8_t picked, uint8_t by_hand, uint8_t from_roll, uint8_t has_edge,
                                     const uint8_t *edge, size_t edge_len, negs_orientation_t o);
const char *negs_pick_prompt(uint32_t pick);
uint8_t negs_negative_same_number(double p, double q);





/* ------------------------------------------------------------------ */
/* Phase 3c: Media helpers: embedded previews, photo facts, tone graph, help (core/src/rules) */
/* ------------------------------------------------------------------ */
/* (Phase 3c's declarations go here.) */





#ifdef __cplusplus
}
#endif

#endif /* NEGS_CORE_H */
