// Phase 4c's structs in negs_core.h: size_of / offset_of by C name
// (see layout.odin): batch, match cache, rendered preview.
package negs_core

import "../caches/preview"

@(private)
layout_4c := []Layout_Entry {
	{"negs_batch_save_t", size_of(Batch_Save_C)},
	{"negs_batch_save_t.bit_depth", i64(offset_of(Batch_Save_C, bit_depth))},
	{"negs_batch_save_t.color_space", i64(offset_of(Batch_Save_C, color_space))},
	{"negs_batch_save_t.jpeg_quality", i64(offset_of(Batch_Save_C, jpeg_quality))},
	{"negs_batch_run_t", size_of(Batch_Run_C)},
	{"negs_batch_run_t.statuses", i64(offset_of(Batch_Run_C, statuses))},
	{"negs_batch_run_t.count", i64(offset_of(Batch_Run_C, count))},
	{"negs_batch_run_t.running", i64(offset_of(Batch_Run_C, running))},
	{"negs_batch_run_t.cancelled", i64(offset_of(Batch_Run_C, cancelled))},
	{"negs_batch_counts_t", size_of(Batch_Counts_C)},
	{"negs_batch_counts_t.total", i64(offset_of(Batch_Counts_C, total))},
	{"negs_batch_counts_t.remaining", i64(offset_of(Batch_Counts_C, remaining))},
	{"negs_batch_counts_t.finished", i64(offset_of(Batch_Counts_C, finished))},
	{"negs_batch_counts_t.exported", i64(offset_of(Batch_Counts_C, exported))},
	{"negs_batch_counts_t.failed", i64(offset_of(Batch_Counts_C, failed))},
	{"negs_batch_counts_t.skipped", i64(offset_of(Batch_Counts_C, skipped))},
	{"negs_batch_counts_t.fraction", i64(offset_of(Batch_Counts_C, fraction))},
	{"negs_batch_counts_t.exported_part", i64(offset_of(Batch_Counts_C, exported_part))},
	{"negs_batch_counts_t.other_part", i64(offset_of(Batch_Counts_C, other_part))},
	{"negs_batch_counts_t.needs_attention", i64(offset_of(Batch_Counts_C, needs_attention))},
	{"negs_preview_frame_t", size_of(preview.Frame)},
	{"negs_preview_frame_t.width", i64(offset_of(preview.Frame, width))},
	{"negs_preview_frame_t.height", i64(offset_of(preview.Frame, height))},
	{"negs_preview_frame_t.photo_width", i64(offset_of(preview.Frame, photo_width))},
	{"negs_preview_frame_t.photo_height", i64(offset_of(preview.Frame, photo_height))},
	{"negs_preview_frame_t.source_width", i64(offset_of(preview.Frame, source_width))},
	{"negs_preview_frame_t.source_height", i64(offset_of(preview.Frame, source_height))},
	{"negs_preview_frame_t.working_width", i64(offset_of(preview.Frame, working_width))},
	{"negs_preview_frame_t.working_height", i64(offset_of(preview.Frame, working_height))},
	{"negs_preview_frame_t.raw", i64(offset_of(preview.Frame, raw))},
	{"negs_preview_frame_t.lens_correction_supported", i64(offset_of(preview.Frame, lens_correction_supported))},
}
