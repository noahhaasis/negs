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
/* (Phase 3b's declarations go here.) */





/* ------------------------------------------------------------------ */
/* Phase 3c: Media helpers: embedded previews, photo facts, tone graph, help (core/src/rules) */
/* ------------------------------------------------------------------ */
/* (Phase 3c's declarations go here.) */





#ifdef __cplusplus
}
#endif

#endif /* NEGS_CORE_H */
