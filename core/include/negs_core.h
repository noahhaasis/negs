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
/* (Phase 3b's declarations go here.) */





/* ------------------------------------------------------------------ */
/* Phase 3c: Media helpers: embedded previews, photo facts, tone graph, help (core/src/rules) */
/* ------------------------------------------------------------------ */
/* (Phase 3c's declarations go here.) */





#ifdef __cplusplus
}
#endif

#endif /* NEGS_CORE_H */
