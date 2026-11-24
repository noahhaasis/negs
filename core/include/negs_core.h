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
#define NEGS_CORE_ABI_VERSION 5u

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
/* windows/src's sheet_rules, tray_rules and compare (and model/compare's
 * pixel helpers), with Rust's floating point to the bit. Nothing here keeps
 * state, takes a handle or returns memory: arrays go in borrowed for the
 * call; results go into the caller's arrays, sized as each function says.
 * Words go into the caller's buffer (`out`, `cap` bytes, not
 * NUL-terminated): the function returns the whole length, so a caller
 * whose buffer was too small calls again with one that long. Optional
 * indices come back as int64_t, -1 for none. Static words (const char *)
 * are NUL-terminated and live as long as the process. */

/* -- Sheet: the film strip and the grid (core/src/rules/sheet) -------- */

typedef struct negs_sheet_constants_t {
    double strip, strip_top, strip_rebate, strip_gap, picture, status, band;
    double strip_dim, strip_hover, strip_pressed, pencil_width, pencil_drop;
    double fallback_ratio, strip_margin;
    double leader_width, leader_height, leader_raised, leader_slant, glide_ms;
    double grid_gap, grid_top, grid_side, grid_bottom, grid_size_min, grid_size_max;
    double grid_default, grid_size_step, grid_margin, sentence, foot;
    double box_margin, selection_outset;
    double fade_ms, fade_rise, wheel_step, wheel_tau_ms;
    double bar_width, bar_wide, bar_inset, bar_min, bar_reach, bar_shown_ms, bar_fade_ms;
    uint64_t prefetch_reach, header_batch;
    uint32_t edited_long_side, render_long_side;
    uint64_t edited_limit, renders_limit;
} negs_sheet_constants_t;

/* A grid row: photographs start..end-1, its top and height. Frames are
 * negs_rect_t. */
typedef struct negs_sheet_row_t {
    size_t start, end;
    double y, height;
} negs_sheet_row_t;

/* One photograph as the developing pump sees it. */
typedef struct negs_sheet_pump_item_t {
    uint8_t look;      /* its look is known */
    uint8_t on_screen;
    uint8_t open;      /* open in the edit view */
    uint8_t done;      /* developing, developed, or failed with this look */
} negs_sheet_pump_item_t;

/* The library's filter. rating: 0 any, 1 at least `stars`, 2 unrated;
 * flags: bit 0 picks, 1 unflagged, 2 rejects (none shows every one);
 * edits: 0 any, 1 edited, 2 unedited. */
typedef struct negs_sheet_filter_t {
    uint8_t rating, stars, flags, edits;
} negs_sheet_filter_t;

#define NEGS_SHEET_PART_WORDS 0u
#define NEGS_SHEET_PART_FILTER 1u
#define NEGS_SHEET_PART_CLEAR 2u
#define NEGS_SHEET_MAX_PARTS 8u

/* A piece of the sentence over the grid: its text is text[offset..+len]
 * of the buffer given; `on` for a filter word while filtered. */
typedef struct negs_sheet_part_t {
    uint32_t kind;
    uint8_t on;
    size_t offset, len;
} negs_sheet_part_t;

#define NEGS_THUMB_STRIP 0u
#define NEGS_THUMB_MEDIUM 1u
#define NEGS_THUMB_LARGE 2u

void negs_sheet_constants(negs_sheet_constants_t *out);
double negs_sheet_reject_opacity(uint8_t in_strip, uint8_t current);
double negs_sheet_strip_opacity(uint8_t current, uint8_t selected, uint8_t rejected, uint8_t hovered, uint8_t pressed);

/* The strip: frames' left edges `xs` and `widths`, `count` each. */
double negs_sheet_kept_ratio(uint8_t has_known, double known, double thumbnail);
double negs_sheet_frame_width(uint8_t has_ratio, double ratio);
/* Writes `count` left edges into `xs`; returns the row's width. */
double negs_sheet_strip_layout(const double *widths, size_t count, double *xs);
double negs_sheet_strip_middle(double x, double width);
void negs_sheet_strip_indices(const double *xs, const double *widths, size_t count, double min_x, double max_x, size_t *start, size_t *end);
int64_t negs_sheet_strip_hit(const double *xs, const double *widths, size_t count, double x);
int64_t negs_sheet_strip_index_at(const double *xs, const double *widths, size_t count, double x);
double negs_sheet_centred(double middle, double visible, double content);
double negs_sheet_hold(double old_middle, double new_middle, double new_width, double scroll, double visible);
double negs_sheet_smoothstep(double t);

/* The grid. negs_sheet_layout writes `count` frames and up to `count` rows
 * (how many into *row_count) and returns the grid's height. */
double negs_sheet_clamp_size(double size);
double negs_sheet_layout(const double *ratios, size_t count, double width, double target, negs_rect_t *frames, negs_sheet_row_t *rows, size_t *row_count);
int64_t negs_sheet_row_of(const negs_sheet_row_t *rows, size_t row_count, size_t frame_count, size_t index);
int64_t negs_sheet_vertical(const negs_rect_t *frames, size_t frame_count, const negs_sheet_row_t *rows, size_t row_count, size_t index, int64_t by);
/* Writes up to `frame_count` indices into `out`; returns how many. */
size_t negs_sheet_indices_in(const negs_rect_t *frames, size_t frame_count, const negs_sheet_row_t *rows, size_t row_count, negs_rect_t rect, size_t *out);
void negs_sheet_indices(const negs_sheet_row_t *rows, size_t row_count, double min_y, double max_y, size_t *start, size_t *end);
int64_t negs_sheet_hit(const negs_rect_t *frames, size_t frame_count, const negs_sheet_row_t *rows, size_t row_count, double x, double y);
int64_t negs_sheet_nearest(const negs_rect_t *frames, size_t frame_count, const negs_sheet_row_t *rows, size_t row_count, double x, double y);
double negs_sheet_centre_row(negs_sheet_row_t row, double height, double viewport);
double negs_sheet_reveal(negs_rect_t frame, double height, double scroll, double viewport);
double negs_sheet_keep_anchor(negs_rect_t frame, double height, double down, double from_top, double viewport);

/* Thumbnails: sizes are NEGS_THUMB_*. */
uint32_t negs_sheet_thumb_pixels(uint32_t size);
uint32_t negs_sheet_thumb_grid(double points, double scale);
uint32_t negs_sheet_thumb_for_rows(double size, double scale);
void negs_sheet_thumb_cache_limits(uint32_t size, size_t *bytes, size_t *count);
size_t negs_sheet_thumbnail_centre(const size_t *on_screen, size_t count, uint8_t has_open, size_t open);
/* `order` (count) gets the positions in `indices`, nearest `centre` first. */
void negs_sheet_outward(const size_t *indices, size_t count, size_t centre, size_t *order);
/* `out` holds 2 × prefetch_reach + 1; returns how many. */
size_t negs_sheet_prefetch_order(size_t count, const size_t *on_screen, size_t on_screen_count, uint8_t has_current, size_t current, size_t *out);
/* `out` holds `count`; returns how many. */
size_t negs_sheet_pump_order(const negs_sheet_pump_item_t *items, size_t count, uint8_t low_power, size_t *out);
size_t negs_sheet_helpers(size_t cores, uint64_t memory, uint8_t low_power);
size_t negs_sheet_readers(size_t cores);

/* The sentence: up to `parts_cap` parts (NEGS_SHEET_MAX_PARTS is enough),
 * their texts one after another in `text`; *text_len gets the texts' whole
 * length. Returns how many parts. */
size_t negs_sheet_sentence(const negs_sheet_filter_t *filter, size_t shown, size_t total, negs_sheet_part_t *parts, size_t parts_cap, uint8_t *text, size_t text_cap, size_t *text_len);
size_t negs_sheet_foot(size_t shown, size_t total, size_t selected, uint8_t filtered, uint8_t *out, size_t cap);
double negs_sheet_size_fraction(double size);
double negs_sheet_size_at(double fraction);

/* The fade and scrolling. negs_sheet_cubic_pieces samples `f` (called on
 * this thread before it returns) and writes max(pieces, 1) × 5 doubles:
 * each piece's start, then c0..c3. */
void negs_sheet_look(double along, double *opacity, double *rise);
double negs_sheet_cubic_bezier(double x1, double y1, double x2, double y2, double t);
void negs_sheet_cubic_pieces(double (*f)(void *user, double t), void *user, double length, size_t pieces, double *out);
double negs_sheet_wheel_glide(double at, double to, double dt_ms);
uint8_t negs_sheet_bar_knob(double scroll, double content, double viewport, double *top, double *length);
double negs_sheet_bar_scroll(double top, double content, double viewport);
double negs_sheet_bar_opacity(double since_ms);

/* -- Tray: the film and paper tray (core/src/rules/tray) -------------- */

typedef struct negs_tray_constants_t {
    double tile, picture, tile_gap, group_gap, title, title_gap, inset, top;
    double name_gap, name_line, row, foot, height, star, star_inset;
    double wheel_notch, glide_ms;
    const char *favourites, *scan, *your_negative, *keep_paper_label;
} negs_tray_constants_t;

/* A stock as the tray lists it, and a group of them (strings UTF-8,
 * borrowed for the call). */
typedef struct negs_tray_entry_t {
    const uint8_t *id;
    size_t id_len;
    const uint8_t *name;
    size_t name_len;
} negs_tray_entry_t;

typedef struct negs_tray_section_t {
    const uint8_t *title;
    size_t title_len;
    const negs_tray_entry_t *stocks;
    size_t count;
    uint8_t favourites;
} negs_tray_section_t;

/* A tile in the row; from negs_tray_places, `stock` points into the
 * sections given. */
typedef struct negs_tray_place_t {
    const uint8_t *stock;
    size_t stock_len;
    uint8_t favourite;
} negs_tray_place_t;

/* What's open when Esc is pressed. */
typedef struct negs_tray_escape_context_t {
    uint8_t alert, typing, tray, cropping, grid, image_only, photograph;
} negs_tray_escape_context_t;

#define NEGS_TRAY_HIT_NONE 0u
#define NEGS_TRAY_HIT_TILE 1u
#define NEGS_TRAY_HIT_STAR 2u

#define NEGS_TRAY_ENDING_NOTHING 0u
#define NEGS_TRAY_ENDING_STEP 1u
#define NEGS_TRAY_ENDING_RESTORE 2u

#define NEGS_TRAY_SUBMIT_KEEP 0u
#define NEGS_TRAY_SUBMIT_TRY_FIRST 1u
#define NEGS_TRAY_CANCEL_CLEAR 0u
#define NEGS_TRAY_CANCEL_GO_BACK 1u

/* Keys in, actions out. */
#define NEGS_TRAY_KEY_ESCAPE 0u
#define NEGS_TRAY_KEY_RETURN 1u
#define NEGS_TRAY_KEY_LEFT 2u
#define NEGS_TRAY_KEY_RIGHT 3u
#define NEGS_TRAY_KEY_OTHER 4u
#define NEGS_TRAY_ACTION_GO_BACK 0u
#define NEGS_TRAY_ACTION_KEEP 1u
#define NEGS_TRAY_ACTION_STEP 2u
#define NEGS_TRAY_ACTION_PASS 3u

/* Esc's order, the first that applies. */
#define NEGS_ESCAPE_ALERT 0u
#define NEGS_ESCAPE_TYPING 1u
#define NEGS_ESCAPE_TRAY 2u
#define NEGS_ESCAPE_CROP 3u
#define NEGS_ESCAPE_GRID 4u
#define NEGS_ESCAPE_IMAGE_ONLY 5u
#define NEGS_ESCAPE_SHOW_GRID 6u
#define NEGS_ESCAPE_NOTHING 7u

void negs_tray_constants(negs_tray_constants_t *out);
uint8_t negs_tray_searching(const uint8_t *query, size_t query_len);
/* Which of `n` matched groups (`counts`: their stocks) show, into `kept`
 * (n), and whether the favourites lead; returns how many. */
size_t negs_tray_sections(const size_t *counts, size_t n, size_t favourite_count, const uint8_t *query, size_t query_len, size_t *kept, uint8_t *favourites_lead);
/* Whether the film row leads with Your negative's Scan group. */
uint8_t negs_tray_with_scan(uint8_t paper, uint8_t read_as, const uint8_t *query, size_t query_len);
/* Every tile, in order, if `cap` holds them; returns how many there are. */
size_t negs_tray_places(const negs_tray_section_t *sections, size_t n, negs_tray_place_t *out, size_t cap);
int64_t negs_tray_step(const uint8_t *current, size_t current_len, uint8_t in_favourites, int32_t offset, const negs_tray_place_t *places, size_t n);
double negs_tray_width(size_t stock_count);
/* `out` holds n. */
void negs_tray_starts(const negs_tray_section_t *sections, size_t n, double *out);
double negs_tray_content_width(const negs_tray_section_t *sections, size_t n);
double negs_tray_tile_left(double start, size_t index);
uint8_t negs_tray_tile_x(const uint8_t *stock, size_t stock_len, size_t section, const negs_tray_section_t *sections, size_t n, double *x);
int64_t negs_tray_section_of(const uint8_t *stock, size_t stock_len, const negs_tray_section_t *sections, size_t n, uint8_t favourites_first);
size_t negs_tray_current(double scrolled_to, const double *starts, size_t n);
double negs_tray_centring(double tile_x, double visible, double content);
double negs_tray_jump(double start, double visible, double content);
double negs_tray_clamp(double x, double visible, double content);
double negs_tray_reveal(double offset, const uint8_t *stock, size_t stock_len, uint8_t in_favourites, uint8_t centre, const negs_tray_section_t *sections, size_t n, double visible);
double negs_tray_wheel(double offset, int32_t delta, uint8_t sideways, double visible, double content);
double negs_tray_glide(double from, double to, double elapsed_ms);
/* NEGS_TRAY_HIT_*, and the tile's section and index. */
uint32_t negs_tray_hit(const negs_tray_section_t *sections, size_t n, double x, double y, size_t *section, size_t *index);
/* x, y, width, height. */
void negs_tray_star_rect(double left, double out[4]);
void negs_tray_stand_in_size(uint32_t image_width, uint32_t image_height, double photo_width, double photo_height, double out[2]);

const char *negs_tray_star_help(uint8_t favourite);
size_t negs_tray_star_label(const uint8_t *name, size_t name_len, uint8_t favourite, uint8_t *out, size_t cap);
const char *negs_tray_tile_value(uint8_t selected, uint8_t before);
size_t negs_tray_index_label(const uint8_t *title, size_t title_len, uint8_t *out, size_t cap);
size_t negs_tray_hint(uint8_t has_before, const uint8_t *before, size_t before_len, uint8_t *out, size_t cap);
size_t negs_tray_matches(size_t count, uint8_t *out, size_t cap);
size_t negs_tray_no_match(const uint8_t *query, size_t query_len, uint8_t paper, uint8_t *out, size_t cap);
const char *negs_tray_done_help(uint8_t paper);
const char *negs_tray_keep_paper_words(uint8_t kept);

/* A visit: the undo name for keeping, and the tiles' Trying and Before. */
const char *negs_tray_visit_action(uint8_t paper, uint8_t read_as);
uint8_t negs_tray_visit_tried(const uint8_t *before_id, size_t before_len, const uint8_t *current, size_t current_len);
uint8_t negs_tray_visit_shows_before(const uint8_t *before_id, size_t before_len, const uint8_t *stock, size_t stock_len, const uint8_t *current, size_t current_len);
/* `unchanged`: the edit before the visit equals the edit now. */
uint32_t negs_tray_ending(uint8_t keep, uint8_t unchanged);
uint32_t negs_tray_submit(const uint8_t *query, size_t query_len);
uint32_t negs_tray_cancel(const uint8_t *query, size_t query_len);
/* NEGS_TRAY_ACTION_*; *step is -1 or 1 for a step. */
uint32_t negs_tray_key(uint32_t key, uint8_t command, uint8_t typing, int32_t *step);
uint32_t negs_tray_escape(const negs_tray_escape_context_t *context);

/* -- Compare (core/src/rules/compare) --------------------------------- */

typedef struct negs_compare_constants_t {
    double caption, label, stacked_caption, gap, stacked_gap, side, bottom;
    const char *parts[5]; /* the camera's facts, as the status line names them */
} negs_compare_constants_t;

/* A string that may be missing. */
typedef struct negs_compare_text_t {
    const uint8_t *text;
    size_t len;
    uint8_t has;
} negs_compare_text_t;

/* A photograph, or one of its duplicates; equal when both are byte for byte. */
typedef struct negs_compare_item_t {
    const uint8_t *photo;
    size_t photo_len;
    negs_compare_text_t variant;
} negs_compare_item_t;

#define NEGS_COMPARE_OFFER_TOO_MANY 0u
#define NEGS_COMPARE_OFFER_PAIR 1u
#define NEGS_COMPARE_OFFER_WITH_CURRENT 2u
#define NEGS_COMPARE_OFFER_WITH_ORIGINAL 3u

typedef struct negs_compare_offer_t {
    uint32_t kind;
    uint8_t enabled, with_original;
} negs_compare_offer_t;

#define NEGS_COMPARE_WORD_PAIR 0u
#define NEGS_COMPARE_WORD_TOO_MANY 1u
#define NEGS_COMPARE_WORD_SELECT_SECOND 2u

#define NEGS_ORIGINAL_HOLD 0u
#define NEGS_ORIGINAL_SPLIT 1u
#define NEGS_ORIGINAL_SIDE_BY_SIDE 2u
#define NEGS_ORIGINAL_RAW 0u
#define NEGS_ORIGINAL_CAMERA_JPEG 1u

#define NEGS_COMPARE_ROW 0u
#define NEGS_COMPARE_COLUMN 1u

/* How the camera's JPEG is laid over the RAW's frame: the picture made is
 * out[0] × out[1]; `same`: the JPEG is stretched to it; else it is resized
 * to size[0] × size[1] and drawn at at[0], at[1] on black. */
typedef struct negs_compare_over_frame_t {
    uint32_t out[2];
    uint32_t size[2];
    int64_t at[2];
    uint8_t same;
} negs_compare_over_frame_t;

void negs_compare_constants(negs_compare_constants_t *out);
uint8_t negs_compare_edited_is_first(uint8_t first_original, uint8_t second_original, uint8_t active_first);
uint8_t negs_compare_against_original(uint8_t first_original, uint8_t second_original);
/* `current_other`: there is a current photograph and it isn't the one
 * clicked. Then the title: a and b the pair's numbers, a the current's
 * number, or a the original's word. */
void negs_compare_offer(size_t target_count, uint8_t current_other, uint8_t can_compare, uint8_t can_with_original, negs_compare_offer_t *out);
size_t negs_compare_offer_title(uint32_t kind, const uint8_t *a, size_t a_len, const uint8_t *b, size_t b_len, uint8_t *out, size_t cap);
uint32_t negs_compare_word(size_t selected, uint8_t can_compare);
size_t negs_compare_word_title(uint32_t kind, const uint8_t *a, size_t a_len, const uint8_t *b, size_t b_len, uint8_t *out, size_t cap);
/* a, b: each photograph's five facts, NULL while unknown. */
size_t negs_compare_differences(const negs_compare_text_t a[5], const negs_compare_text_t b[5], const uint8_t *film_a, size_t film_a_len, const uint8_t *film_b, size_t film_b_len, negs_compare_text_t paper_a, negs_compare_text_t paper_b, uint8_t *out, size_t cap);
const char *negs_compare_menu_title(uint8_t comparing);
/* The index into `order`, or -1. */
int64_t negs_compare_step(const negs_compare_item_t *from, const negs_compare_item_t *first, const negs_compare_item_t *order, size_t n, int64_t offset);
size_t negs_compare_frame_number(const negs_compare_item_t *items, size_t n, const negs_compare_item_t *item, uint8_t *out, size_t cap);

const char *negs_compare_original_mode_key(uint32_t mode);
uint32_t negs_compare_original_mode_from_key(const uint8_t *key, size_t key_len);
const char *negs_compare_original_mode_title(uint32_t mode);
size_t negs_compare_original_mode_help(uint32_t mode, const uint8_t *original, size_t original_len, uint8_t *out, size_t cap);
const char *negs_compare_original_reference_key(uint32_t reference);
uint32_t negs_compare_original_reference_from_key(const uint8_t *key, size_t key_len);
const char *negs_compare_original_reference_title(uint32_t reference);

/* Geometry: frames are negs_rect_t (x, y, width, height). */
void negs_compare_fit(double aspect, double room_width, double room_height, double out[2]);
void negs_compare_row_layout(double canvas_width, double canvas_height, double first_aspect, double second_aspect, negs_rect_t out[2]);
void negs_compare_column_layout(double canvas_width, double canvas_height, double first_aspect, double second_aspect, negs_rect_t out[2]);
/* NEGS_COMPARE_ROW or _COLUMN, whichever shows the two larger. */
uint32_t negs_compare_layout(double canvas_width, double canvas_height, double first_aspect, double second_aspect, negs_rect_t out[2]);
void negs_compare_slider_frame(double canvas_width, double canvas_height, double aspect, negs_rect_t *out);

/* Pixels: RGBA rows packed. negs_compare_halved writes
 * max(width / 2, 1) × max(height / 2, 1) pixels, each the mean of four;
 * width and height at least 1 (else nothing is written). */
void negs_compare_halved(const uint8_t *src, uint32_t width, uint32_t height, uint8_t *out);
void negs_compare_over_frame(uint32_t width, uint32_t height, uint32_t frame_width, uint32_t frame_height, negs_compare_over_frame_t *out);





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
/* Embedded previews (core/src/rules/embedded; windows/src/embedded_preview.rs):
 * a RAW's JPEG previews found from its TIFF IFDs (or RAF header) without
 * decoding anything. */

typedef struct negs_jpeg_t {
    uint64_t offset;
    uint64_t length;
    uint32_t width;
    uint32_t height;
} negs_jpeg_t;

/* The JPEGs in the order the IFDs name them, each offset once, in one
 * block the caller frees with negs_previews_free; orientation 1-8 when
 * has_orientation. */
typedef struct negs_previews_t {
    negs_jpeg_t *jpegs;
    size_t count;
    uint16_t orientation;
    uint8_t has_orientation;
    uint8_t reserved[5];
} negs_previews_t;

/* A header in memory: 1 when it's a TIFF or RAF header (count may be 0),
 * 0 when it isn't. */
uint8_t negs_embedded_find_bytes(const uint8_t *data, size_t len, negs_previews_t *out);
/* A file, read a piece at a time: 1 when it has previews, 0 when it has
 * none this finds or can't be read. */
uint8_t negs_embedded_find(const uint8_t *path, size_t path_len, negs_previews_t *out);
/* Frees the block and zeroes *previews; a zeroed one is fine. */
void negs_previews_free(negs_previews_t *previews);
/* The `length` bytes at `offset` in the file into *out (empty for 0); 0
 * when they aren't all there or can't be read. */
uint8_t negs_embedded_bytes(const uint8_t *path, size_t path_len, uint64_t offset, uint64_t length, negs_buf_t *out);
/* The same into the caller's memory, so a host that owns the bytes copies
 * nothing: with dst NULL, whether the file has those bytes (1) or not (0);
 * else reads them into dst, which holds `length` bytes. */
uint8_t negs_embedded_read(const uint8_t *path, size_t path_len, uint64_t offset, uint64_t length, uint8_t *dst);
/* Previews::largest (by pixels, then length; the last of equals),
 * ::for_size (the smallest of the largest's shape at least `pixels` on its
 * long side, else the largest) and ::ratio (width / height upright): 0
 * when there are no JPEGs. */
uint8_t negs_previews_largest(const negs_jpeg_t *jpegs, size_t count, negs_jpeg_t *out);
uint8_t negs_previews_for_size(const negs_jpeg_t *jpegs, size_t count, uint32_t pixels, negs_jpeg_t *out);
uint8_t negs_previews_ratio(const negs_jpeg_t *jpegs, size_t count, uint16_t orientation, uint8_t has_orientation, double *out);

/* Photo facts (core/src/rules/facts; windows/src/photo_facts.rs): the
 * caption's camera, ISO, shutter, aperture and focal length. Reading the
 * EXIF is the engine's; the host passes its values in. Text is UTF-8;
 * `decimal` is a Unicode scalar value. */

/* camera is borrowed going in; from negs_photo_facts_from_exif it points
 * into the block that call returns. */
typedef struct negs_photo_facts_t {
    const uint8_t *camera;
    size_t camera_len;
    uint8_t has_camera;
    uint8_t has_iso;
    uint8_t has_exposure;     /* seconds */
    uint8_t has_aperture;
    uint8_t has_focal_length; /* millimetres */
    uint8_t reserved[3];
    double iso;
    double exposure;
    double aperture;
    double focal_length;
} negs_photo_facts_t;

/* The EXIF values PhotoFacts::read uses; rationals are {n, d}. */
typedef struct negs_exif_facts_t {
    const uint8_t *make;
    size_t make_len;
    const uint8_t *model;
    size_t model_len;
    uint8_t has_make;
    uint8_t has_model;
    uint8_t has_iso_speed_ratings;
    uint8_t has_iso_speed;
    uint8_t has_exposure_time;
    uint8_t has_fnumber;
    uint8_t has_focal_length;
    uint8_t reserved;
    uint16_t iso_speed_ratings;
    uint16_t reserved2;
    uint32_t iso_speed;
    uint32_t exposure_time[2];
    uint32_t fnumber[2];
    uint32_t focal_length[2];
} negs_exif_facts_t;

/* What the camera functions give: none, some, or 2 where the Rust original
 * panics (a model starting with the maker's name, lowercased, cut off a
 * char boundary). */
#define NEGS_FACTS_NONE 0u
#define NEGS_FACTS_SOME 1u
#define NEGS_FACTS_PANICS 2u

/* "Nikon Z f" from "NIKON CORPORATION" and "NIKON Z f": the text in *out
 * when NEGS_FACTS_SOME. */
uint8_t negs_photo_facts_camera(const uint8_t *make, size_t make_len, uint8_t has_make,
                                const uint8_t *model, size_t model_len, uint8_t has_model, negs_buf_t *out);
/* PhotoFacts::read's sums on the EXIF's values: *out, its camera in
 * *camera (free it); NEGS_FACTS_SOME, or NEGS_FACTS_PANICS. */
uint8_t negs_photo_facts_from_exif(const negs_exif_facts_t *exif, negs_photo_facts_t *out, negs_buf_t *camera);
/* "1/1250", "2 s", "1,3 s". */
negs_buf_t negs_photo_facts_shutter(double seconds, uint32_t decimal);
/* The caption's parts (camera, ISO, shutter, aperture, focal length), one
 * after another in the block; lengths[i] is SIZE_MAX for a part absent. */
negs_buf_t negs_photo_facts_parts(const negs_photo_facts_t *facts, uint32_t decimal, size_t lengths[5]);

/* Tone graph (core/src/rules/tone; windows/src/tone_graph.rs): the print
 * curves' graph maths. Series are borrowed for the call; a channel is 0-2,
 * or -1 for the luma (brightness) or mean (paper exposure) of all three.
 * The caller makes sure every series indexed is long enough: rgb and paper
 * channels at least as long as stops (or, for -1, as rgb[0] / paper[0]). */

typedef struct negs_tone_series_t {
    const double *data;
    size_t len;
} negs_tone_series_t;

/* A Curve's or a Display's series. */
typedef struct negs_tone_curve_t {
    negs_tone_series_t stops;    /* mean film exposure per grey, in stops */
    negs_tone_series_t rgb[3];   /* the scan per channel */
    negs_tone_series_t paper[3]; /* log10 paper exposure, when has_paper */
    uint8_t has_paper;
    uint8_t reserved[7];
} negs_tone_curve_t;

typedef struct negs_tone_controls_t {
    double print_exposure;
    double gamma;
    double fast;
    double slow;
} negs_tone_controls_t;

typedef struct negs_tone_geometry_t {
    double width;
    double height;
    double inset;
} negs_tone_geometry_t;

#define NEGS_TONE_MIDDLE 0u
#define NEGS_TONE_SHADOWS 1u
#define NEGS_TONE_HIGHLIGHTS 2u
#define NEGS_TONE_CURVE 3u

/* What a drag sets. */
#define NEGS_TONE_SET_NONE 0u
#define NEGS_TONE_SET_PRINT_EXPOSURE 1u
#define NEGS_TONE_SET_GAMMA 2u
#define NEGS_TONE_SET_FAST 3u
#define NEGS_TONE_SET_SLOW 4u

typedef struct negs_tone_point_t {
    uint32_t kind; /* NEGS_TONE_SHADOWS, _MIDDLE, _HIGHLIGHTS */
    uint32_t reserved;
    double x;
    double y;
} negs_tone_point_t;

double negs_tone_index_of_decreasing(const double *series, size_t len, double value);
double negs_tone_index_of_increasing(const double *series, size_t len, double value);
double negs_tone_interpolate(const double *series, size_t len, double index);
double negs_tone_luma(double r, double g, double b);
/* three[0].len values into out: the luma (luma 1) or the mean of three. */
void negs_tone_combine(const negs_tone_series_t three[3], uint8_t luma, double *out);
/* Curve::predicted into rgb_out / paper_out (copies of the curve's rgb and
 * paper, the first stops.len values overwritten); 0 when there's nothing
 * to predict (no paper, or the same controls). */
uint8_t negs_tone_predict(const negs_tone_curve_t *curve, negs_tone_controls_t controls, negs_tone_controls_t next,
                          double *const rgb_out[3], double *const paper_out[3]);
/* Display::handles: shadows, middle, highlights in stops; 0 when the paper
 * exposure never crosses 0. */
uint8_t negs_tone_handles(const negs_tone_curve_t *display, int32_t channel, double out[3]);
/* Display::value: `series` at `stops` film exposure. */
double negs_tone_value(const negs_tone_curve_t *display, const double *series, size_t len, double stops);
double negs_tone_x(negs_tone_geometry_t geometry, double stops);
double negs_tone_stops(negs_tone_geometry_t geometry, double x);
double negs_tone_y(negs_tone_geometry_t geometry, double brightness);
double negs_tone_brightness(negs_tone_geometry_t geometry, double y);
/* 0 or 3 points. */
size_t negs_tone_handle_points(negs_tone_geometry_t geometry, const negs_tone_curve_t *display, int32_t channel,
                               negs_tone_point_t out[3]);
/* Geometry::hit: 1 and *kind (and *tone for NEGS_TONE_CURVE), or 0. */
uint8_t negs_tone_hit(negs_tone_geometry_t geometry, const negs_tone_curve_t *display, int32_t channel,
                      double x, double y, double radius, double tolerance, uint32_t *kind, double *tone);
/* Drag::start's tone for a handle (`tone` is NEGS_TONE_CURVE's own). */
uint8_t negs_tone_drag_tone(uint32_t kind, double tone, const negs_tone_curve_t *display, int32_t channel, double *out);
/* Drag::value: NEGS_TONE_SET_* and the snapped value in *out. */
uint32_t negs_tone_drag_value(uint32_t kind, double tone, const negs_tone_curve_t *base,
                              negs_tone_controls_t start, negs_tone_controls_t current, int32_t channel,
                              negs_tone_geometry_t geometry, double x, double y, double *out);
/* state::snap (low <= high). */
double negs_tone_snap(double value, double low, double high, double step);
/* Handle::title and ::hint: static. */
const char *negs_tone_handle_title(uint32_t kind);
const char *negs_tone_handle_hint(uint32_t kind);

/* Help (core/src/rules/help; windows/src/help.rs): the inspector's
 * tooltips. Texts come back as static (pointer, length), not
 * NUL-terminated. */

typedef struct negs_help_str_t {
    const uint8_t *data;
    size_t len;
} negs_help_str_t;

/* A control's tooltip by parameter path; a grain lane shares its array's.
 * The host passes negative::SCAN_ILLUMINANT's path and
 * state::GRAIN_STOCK_FIELDS (at most 16). 0 when it has none. */
uint8_t negs_help_text(const uint8_t *path, size_t path_len, const uint8_t *scan_illuminant, size_t scan_len,
                       const negs_help_str_t *grain_fields, size_t grain_count, negs_help_str_t *out);
/* A group's tooltip by title. */
uint8_t negs_help_group(const uint8_t *title, size_t title_len, negs_help_str_t *out);
/* 0 SCAN_ILLUMINANT, 1 FILM, 2 PAPER, 3 PAPER_LOCK, 4 PAPER_KEPT. */
uint8_t negs_help_constant(uint32_t which, negs_help_str_t *out);
/* Row `index` of table 0 (paths) or 1 (groups); 0 past its end. */
uint8_t negs_help_row(uint32_t table, size_t index, negs_help_str_t *key, negs_help_str_t *text);

/* ------------------------------------------------------------------ */
/* Phase 4 base: value, edit state (core/src/value, core/src/state)     */
/* ------------------------------------------------------------------ */
/* JSON as serde_json 1.0 (float_roundtrip) reads and writes it, and
 * windows/src/state.rs's EditState, Roll and BrightnessMatch. Everything
 * crosses as UTF-8 JSON text, borrowed for the call: (pointer, length),
 * not NUL-terminated. A text the core can't read as `serde_json::from_slice`
 * would makes the call return 0 (or NULL) with an empty `out`. Results are
 * owned negs_buf_t (free with negs_buf_free; `out` may be NULL to drop
 * one), the caller's buffer as in Phase 3a (`out`, `cap`; returns the whole
 * length), or static words (const char *, NUL-terminated, never freed).
 *
 * Text written here is byte for byte serde_json's: compact is
 * `to_string`, NEGS_JSON_PRETTY `to_string_pretty`; objects come out with
 * their keys sorted (serde_json's Value is a BTreeMap), except an edit
 * state's own fields, which keep EditState's declaration order unless
 * NEGS_JSON_SORTED asks for `to_value`'s order (as inside `json!`). */

#define NEGS_JSON_PRETTY 1u
#define NEGS_JSON_SORTED 2u

/* A borrowed UTF-8 string, for lists the host passes. */
typedef struct negs_str_t {
    const uint8_t *data;
    size_t len;
} negs_str_t;

/* The text read and written again (canonical: sorted keys, serde's
 * number kinds and zmij doubles). 1 if it is JSON. */
uint8_t negs_value_rewrite(const uint8_t *text, size_t len, uint32_t flags, negs_buf_t *out);
/* serde_json's text for a double (zmij: "1.0", "1e+16", "1.5e-7"; "null"
 * when not finite). */
size_t negs_value_write_f64(double v, uint8_t *out, size_t cap);
/* Rust's `format!("{v}")` for a double: shortest digits, no exponent,
 * "-0", "NaN", "inf". At most 330 bytes. */
size_t negs_value_display_f64(double v, uint8_t *out, size_t cap);

/* windows/src/json.rs. `get` writes the value at `path` ("a.b.0"), null
 * when missing; `set` writes the whole tree with `item` (JSON) put there,
 * as json::set does (missing keys become objects; an index past an
 * array's end or a scalar on the way changes nothing). */
uint8_t negs_json_get(const uint8_t *value, size_t len, const uint8_t *path, size_t path_len, negs_buf_t *out);
uint8_t negs_json_set(const uint8_t *value, size_t len, const uint8_t *path, size_t path_len, const uint8_t *item,
                      size_t item_len, negs_buf_t *out);
/* json::number (0 for what isn't a number), json::boolean, json::string
 * (the string's bytes, empty when it isn't one). Text that isn't JSON
 * counts as null. */
double negs_json_number(const uint8_t *value, size_t len);
uint8_t negs_json_boolean(const uint8_t *value, size_t len);
uint8_t negs_json_string(const uint8_t *value, size_t len, negs_buf_t *out);
/* json::num's value as JSON text: an integer when `v` is a whole number
 * below 9e15, else a double; "null" when not finite. */
size_t negs_json_num(double v, uint8_t *out, size_t cap);
/* json::same: 1 or 0; 2 when either text isn't JSON. */
uint8_t negs_json_same(const uint8_t *a, size_t a_len, const uint8_t *b, size_t b_len);
/* json::restoring(defaults, saved, nullable). `nullable` is borrowed. */
uint8_t negs_json_restoring(const uint8_t *defaults, size_t defaults_len, const uint8_t *saved, size_t saved_len,
                            const negs_str_t *nullable, size_t nullable_count, negs_buf_t *out);

/* An edit state, parsed once: what `serde_json::from_slice::<EditState>`
 * reads (NULL where that fails). The caller owns it until negs_state_free;
 * the core never keeps it. Handles share nothing: one is used from one
 * thread at a time, different ones from different threads. */
typedef struct negs_state_t negs_state_t;

negs_state_t *negs_state_parse(const uint8_t *json, size_t len);
/* EditState::default(). */
negs_state_t *negs_state_default(void);
void negs_state_free(negs_state_t *state);
/* serde_json::to_string(&state) (flags: NEGS_JSON_PRETTY, NEGS_JSON_SORTED).
 * An empty buf for NULL. */
negs_buf_t negs_state_write(const negs_state_t *state, uint32_t flags);
/* EditState's PartialEq. */
uint8_t negs_state_eq(const negs_state_t *a, const negs_state_t *b);
/* EditState::rendered(): a new handle the caller frees. */
negs_state_t *negs_state_rendered(const negs_state_t *state);
/* EditState::match_inputs() as JSON (sorted keys, as json! writes it). */
negs_buf_t negs_state_match_inputs(const negs_state_t *state);
int32_t negs_state_rotation(const negs_state_t *state);
uint8_t negs_state_flip(const negs_state_t *state);
void negs_state_orientation(const negs_state_t *state, negs_orientation_t *out);
/* EditState::size_target(): 1 and `out` when it has one. */
uint8_t negs_state_size_target(const negs_state_t *state, uint32_t *out);
/* Roll::filtered for a roll with these shifts. */
uint8_t negs_state_roll_filtered(const negs_state_t *state, double roll_magenta, double roll_yellow);
/* Roll::own_filtration: the roll's paper (has_paper 0 for None) against
 * the state's `paper`. Static. */
const char *negs_state_roll_own_filtration(const uint8_t *roll_paper, size_t len, uint8_t has_paper,
                                           const uint8_t *paper, size_t paper_len);

/* The same rules on the JSON of one value (what the state holds at the
 * path), for hosts that keep the state themselves: io.rotate_cw, io.flip,
 * the size target's path, a whole params object, the two enlarger shifts.
 * Text that isn't JSON counts as null. */
int32_t negs_state_rotation_of(const uint8_t *value, size_t len);
uint8_t negs_state_flip_of(const uint8_t *value, size_t len);
uint8_t negs_state_size_target_of(const uint8_t *value, size_t len, uint32_t *out);
uint8_t negs_state_params_size_target(const uint8_t *params, size_t len, uint32_t *out);
uint8_t negs_state_roll_filtered_of(double roll_magenta, double roll_yellow, const uint8_t *magenta, size_t magenta_len,
                                    const uint8_t *yellow, size_t yellow_len);
/* EditState::restored_params(defaults, saved, nullable) as JSON. */
uint8_t negs_state_restored_params(const uint8_t *defaults, size_t defaults_len, const uint8_t *saved,
                                   size_t saved_len, const negs_str_t *nullable, size_t nullable_count,
                                   negs_buf_t *out);
/* 0 PREP_EXPOSURE, 1 FLIP, 2 SIZE_TARGET, 3 SIZE_FACTOR, 4 LENS_CORRECTION,
 * 5 the default film, 6 the default paper, 7 and 8 Roll::own_filtration's
 * two words. Static; NULL past them. */
const char *negs_state_constant(uint32_t which);

/* ------------------------------------------------------------------ */
/* Phase 4a: state rules, edits, inspector                              */
/* ------------------------------------------------------------------ */

/* ------------------------------------------------------------------ */
/* Phase 4b: presets, session                                           */
/* ------------------------------------------------------------------ */

/* ------------------------------------------------------------------ */
/* Phase 4c: batch, match cache, rendered preview                       */
/* ------------------------------------------------------------------ */

/* std::path's rules, as the batch and the caches use them. `style`: 0 this
 * platform's, 1 Unix, 2 Windows. `op`: 0 file_name, 1 file_stem,
 * 2 extension, 3 parent (0 for None), 4 with_extension(path, arg),
 * 5 join(path, arg), 6 display() (String::from_utf8_lossy). */
uint8_t negs_caches_path(uint32_t op, uint32_t style, const uint8_t *path, size_t path_len, const uint8_t *arg,
                         size_t arg_len, negs_buf_t *out);
/* Duration::as_secs_f64. */
double negs_caches_secs_f64(uint64_t secs, uint32_t nanos);

/* Batch export (windows/src/batch.rs). Kinds by table: 0 Naming's keys,
 * 1 its titles, 2 Existing's keys, 3 its titles, 4 Status's keys,
 * 5 OutputFormat's keys, 6 Edge's keys, 7 ExportEngine's keys,
 * 8 OutputFormat's extensions; each in its ALL order. Static; NULL past
 * them. negs_batch_from_key: the index of `key` in table 0, 2, 5, 6 or 7,
 * else the default (0; ExportEngine::from_key: Fast unless "reference"). */
const char *negs_batch_key(uint32_t table, uint32_t index);
uint32_t negs_batch_from_key(uint32_t table, const uint8_t *key, size_t len);

/* SaveSettings. */
typedef struct negs_batch_save_t {
    uint32_t bit_depth;
    uint64_t color_space;
    uint32_t jpeg_quality;
} negs_batch_save_t;

/* Options::default() as serde_json writes it. */
negs_buf_t negs_batch_options_default(void);
/* Options::save() for these choices. */
void negs_batch_options_save(const uint8_t *format, size_t len, uint32_t bit_depth, uint64_t color_space,
                             uint32_t jpeg_quality, negs_batch_save_t *out);
/* Options::file_name: `naming` a Naming index, `format` the format's key. */
negs_buf_t negs_batch_file_name(uint32_t naming, const uint8_t *custom, size_t custom_len, const uint8_t *name,
                                size_t name_len, const uint8_t *film, size_t film_len, const uint8_t *format,
                                size_t format_len);
/* batch_name(photo, title), by this platform's path rules. */
negs_buf_t negs_batch_name(const uint8_t *photo, size_t photo_len, const uint8_t *title, size_t title_len);
/* numbered's `n`th candidate, "{stem} {n}.{ext}": the host asks for
 * n = 2, 3, ... until one isn't taken. */
negs_buf_t negs_batch_numbered_name(const uint8_t *file_name, size_t len, uint64_t n);
/* unique(names) as a JSON array of strings. `names` is borrowed. */
uint8_t negs_batch_unique(const negs_str_t *names, size_t count, negs_buf_t *out);
/* matched's exposure: `exposure` the JSON at params.exposure.exposure
 * (null when absent), `old` the state's match (has_old 0 for none), `ev`
 * the new one. 1 and the new total's JSON when it moved by 0.05 EV or
 * more; 0 when nothing changes. */
uint8_t negs_batch_matched_exposure(const uint8_t *exposure, size_t len, uint8_t has_old, double old, double ev,
                                    negs_buf_t *total);
/* matched(state, ev, limited, inputs) on a state's JSON: 1 and the
 * matched state's JSON, 0 for None (or a state serde wouldn't read). */
uint8_t negs_batch_matched(const uint8_t *state, size_t len, double ev, uint8_t limited, const uint8_t *inputs,
                           size_t inputs_len, negs_buf_t *out);

/* A batch's photographs by status (Status indices), borrowed. */
typedef struct negs_batch_run_t {
    const uint8_t *statuses;
    size_t count;
    uint8_t running;
    uint8_t cancelled;
} negs_batch_run_t;

/* Run's counts: total, the waiting, finished(), exported(), failures,
 * skipped(), fraction(), rule_parts() and needs_attention(). */
typedef struct negs_batch_counts_t {
    uint64_t total;
    uint64_t remaining;
    uint64_t finished;
    uint64_t exported;
    uint64_t failed;
    uint64_t skipped;
    double fraction;
    double exported_part;
    double other_part;
    uint8_t needs_attention;
} negs_batch_counts_t;

void negs_batch_run_counts(const negs_batch_run_t *run, negs_batch_counts_t *out);
/* Run::title, ::outcome_words (1 and `failed` when it has the second
 * part), ::finished_words (with the destination folder), ::progress_words
 * (with the Lightroom key, has_lightroom 0 for None). */
negs_buf_t negs_batch_run_title(const negs_batch_run_t *run);
uint8_t negs_batch_run_outcome(const negs_batch_run_t *run, negs_buf_t *words, negs_buf_t *failed);
negs_buf_t negs_batch_run_finished(const negs_batch_run_t *run, const uint8_t *destination, size_t len);
negs_buf_t negs_batch_run_progress(const negs_batch_run_t *run, const uint8_t *lightroom, size_t len,
                                   uint8_t has_lightroom);
/* lightroom_title(key). Static. */
const char *negs_batch_lightroom_title(const uint8_t *key, size_t len);
/* exporting(finished, total, lightroom): `lightroom` the title. */
negs_buf_t negs_batch_exporting(uint64_t finished, uint64_t total, const uint8_t *lightroom, size_t len,
                                uint8_t has_lightroom);
/* message(count, engine, ExportSize { edge, pixels }, save, preview_space):
 * engine an ExportEngine index, edge an Edge index. */
negs_buf_t negs_batch_message(uint64_t count, uint32_t engine, uint32_t edge, uint32_t pixels,
                              const negs_batch_save_t *save, const uint8_t *preview_space, size_t len);

/* The journal (active-export.json). negs_batch_journal_rewrite: the
 * journal serde would read from `text`, as serde_json writes it (0 when it
 * isn't one). negs_batch_journal_write: Journal::write of the journal in
 * `json`: staged beside `path` and moved over it; 0 written, 1 failed
 * (`error` says why), 2 `json` isn't a journal the core reads (a NaN in
 * it: write the host's bytes with negs_batch_journal_write_bytes, which
 * stages them the same way). negs_batch_journal_read: Journal::read, as
 * JSON (0 for None). */
uint8_t negs_batch_journal_rewrite(const uint8_t *text, size_t len, negs_buf_t *out);
uint8_t negs_batch_journal_write(const uint8_t *path, size_t path_len, const uint8_t *json, size_t len,
                                 negs_buf_t *error);
uint8_t negs_batch_journal_write_bytes(const uint8_t *path, size_t path_len, const uint8_t *bytes, size_t len,
                                       negs_buf_t *error);
uint8_t negs_batch_journal_read(const uint8_t *path, size_t len, negs_buf_t *out);
void negs_batch_journal_remove(const uint8_t *path, size_t len);

/* The brightness-match cache (windows/src/match_cache.rs).
 * negs_match_key: key(file, film, paper, params, engine), 64 hex digits
 * (0 when the file can't be stamped). negs_match_engine_stamp:
 * engine_stamp(version, helper, data). */
uint8_t negs_match_key(const uint8_t *file, size_t file_len, const uint8_t *film, size_t film_len,
                       const uint8_t *paper, size_t paper_len, const uint8_t *params, size_t params_len,
                       const uint8_t *engine, size_t engine_len, negs_buf_t *out);
negs_buf_t negs_match_engine_stamp(const uint8_t *version, size_t version_len, const uint8_t *helper,
                                   size_t helper_len, const uint8_t *data, size_t data_len);
/* MatchCache's LIMIT. */
uint64_t negs_match_cache_limit(void);

/* One MatchCache: entries loaded from `file` on first use, the least
 * recently used dropped past `limit`. The caller owns it until
 * negs_match_cache_free and locks it: one thread at a time. `now` is the
 * host's clock, seconds since 1970. */
typedef struct negs_match_cache_t negs_match_cache_t;

negs_match_cache_t *negs_match_cache_new(const uint8_t *file, size_t len, uint64_t limit);
void negs_match_cache_free(negs_match_cache_t *cache);
/* solved(key): 1 with `ev` and `limited`, touching the entry; 0 when it
 * has none. */
uint8_t negs_match_cache_solved(negs_match_cache_t *cache, const uint8_t *key, size_t len, double now, double *ev,
                                uint8_t *limited);
/* store(key, ev, limited): 1 when the host should schedule a write (none
 * was scheduled). */
uint8_t negs_match_cache_store(negs_match_cache_t *cache, const uint8_t *key, size_t len, double ev, uint8_t limited,
                               double now);
/* flush's locked half: 1 and the entries file's bytes when dirty (marking
 * it clean); 0 when there's nothing to write. negs_match_cache_write: the
 * unlocked half, staged and moved over `file`; failures ignored. */
uint8_t negs_match_cache_flush_bytes(negs_match_cache_t *cache, negs_buf_t *out);
void negs_match_cache_write(const uint8_t *file, size_t file_len, const uint8_t *bytes, size_t len);
uint64_t negs_match_cache_count(negs_match_cache_t *cache);

/* The rendered preview (windows/src/rendered_preview.rs). */
#define NEGS_PREVIEW_RENDERED 1u

/* Frame without its pixels. */
typedef struct negs_preview_frame_t {
    uint32_t width;
    uint32_t height;
    double photo_width;
    double photo_height;
    double source_width;
    double source_height;
    double working_width;
    double working_height;
    uint8_t raw;
    uint8_t lens_correction_supported;
} negs_preview_frame_t;

/* key(source, helper, data, state): `state` an EditState's JSON, or with
 * NEGS_PREVIEW_RENDERED the JSON of its rendered() value already. 0 when a
 * file can't be stamped (no birth time either), 2 when the core doesn't
 * read the state (a NaN serde wrote as null: send its rendered() value). */
uint8_t negs_preview_key(const uint8_t *source, size_t source_len, const uint8_t *helper, size_t helper_len,
                         const uint8_t *data, size_t data_len, const uint8_t *state, size_t state_len, uint32_t flags,
                         negs_buf_t *out);
/* checksum: DefaultHasher (SipHash-1-3, zero keys) over the bytes as
 * `[u8]::hash` feeds them. */
uint64_t negs_preview_checksum(const uint8_t *bytes, size_t len);
/* read(file, key): 0 with the frame, its display space and the whole file
 * (the pixels from `pixels_offset`); else why: 1 io, 2 size, 3 header,
 * 4 header not serde's, 5 not this key or size, 6 photograph size,
 * 7 pixels. */
uint8_t negs_preview_read(const uint8_t *file, size_t file_len, const uint8_t *key, size_t key_len,
                          negs_preview_frame_t *frame, negs_buf_t *display_space, negs_buf_t *file_bytes,
                          size_t *pixels_offset);
/* write(file, key, frame): 0, or the OS error (-1 a NUL in the path). */
int32_t negs_preview_write(const uint8_t *file, size_t file_len, const uint8_t *key, size_t key_len,
                           const negs_preview_frame_t *frame, const uint8_t *display_space, size_t display_len,
                           const uint8_t *pixels, size_t len);

#ifdef __cplusplus
}
#endif

#endif /* NEGS_CORE_H */
