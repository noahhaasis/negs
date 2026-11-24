/* Phase 4c's structs: batch, match cache, rendered preview. */
#include "layout_check.h"

void check_4c(void) {
    SIZE(negs_batch_save_t);
    FIELD(negs_batch_save_t, bit_depth);
    FIELD(negs_batch_save_t, color_space);
    FIELD(negs_batch_save_t, jpeg_quality);
    SIZE(negs_batch_run_t);
    FIELD(negs_batch_run_t, statuses);
    FIELD(negs_batch_run_t, count);
    FIELD(negs_batch_run_t, running);
    FIELD(negs_batch_run_t, cancelled);
    SIZE(negs_batch_counts_t);
    FIELD(negs_batch_counts_t, total);
    FIELD(negs_batch_counts_t, remaining);
    FIELD(negs_batch_counts_t, finished);
    FIELD(negs_batch_counts_t, exported);
    FIELD(negs_batch_counts_t, failed);
    FIELD(negs_batch_counts_t, skipped);
    FIELD(negs_batch_counts_t, fraction);
    FIELD(negs_batch_counts_t, exported_part);
    FIELD(negs_batch_counts_t, other_part);
    FIELD(negs_batch_counts_t, needs_attention);
    SIZE(negs_preview_frame_t);
    FIELD(negs_preview_frame_t, width);
    FIELD(negs_preview_frame_t, height);
    FIELD(negs_preview_frame_t, photo_width);
    FIELD(negs_preview_frame_t, photo_height);
    FIELD(negs_preview_frame_t, source_width);
    FIELD(negs_preview_frame_t, source_height);
    FIELD(negs_preview_frame_t, working_width);
    FIELD(negs_preview_frame_t, working_height);
    FIELD(negs_preview_frame_t, raw);
    FIELD(negs_preview_frame_t, lens_correction_supported);
}
