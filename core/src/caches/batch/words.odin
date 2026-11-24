// A batch as the status line and its popover show it (`Run`), and the
// batch dialog's message.
package batch

import "core:fmt"
import "core:strings"
import fs "../../platform/fs"

// A run's photographs by how they went, and whether it runs.
Run :: struct {
	total:     u64,
	waiting:   u64,
	done:      u64,
	failed:    u64,
	skipped:   u64,
	running:   bool,
	cancelled: bool,
}

run_of :: proc(statuses: []Status, running, cancelled: bool) -> Run {
	r := Run{total = u64(len(statuses)), running = running, cancelled = cancelled}
	for s in statuses {
		switch s {
		case .Waiting:
			r.waiting += 1
		case .Done:
			r.done += 1
		case .Failed:
			r.failed += 1
		case .Skipped:
			r.skipped += 1
		}
	}
	return r
}

// Exported, failed or skipped (`Run::finished`).
finished :: proc "contextless" (r: Run) -> u64 {
	return r.total - r.waiting
}

fraction :: proc "contextless" (r: Run) -> f64 {
	return 1.0 if r.total == 0 else f64(finished(r)) / f64(r.total)
}

// The popover's rule: the share exported, then the share that failed or
// was skipped. A batch that all failed draws no yellow.
rule_parts :: proc "contextless" (r: Run) -> (exported: f64, other: f64) {
	if r.total == 0 {
		return 0, 0
	}
	exported = f64(r.done) / f64(r.total)
	other = fraction(r) - exported
	// f64::max: a NaN on one side gives the other.
	if !(other >= 0) {
		other = 0
	}
	return
}

// Ended with something to look at: failures, or a cancel.
needs_attention :: proc "contextless" (r: Run) -> bool {
	return !r.running && (r.failed > 0 || r.cancelled)
}

// The popover's title.
title :: proc(r: Run, allocator := context.allocator) -> string {
	if r.running {
		return fmt.aprintf("Exporting %d of %d", min(finished(r) + 1, r.total), r.total, allocator = allocator)
	}
	if r.cancelled {
		return fmt.aprintf("Cancelled after %d of %d", r.done, r.total, allocator = allocator)
	}
	if r.total == 1 && r.done == 1 {
		return strings.clone("Exported 1 photograph", allocator)
	}
	return fmt.aprintf("Exported %d of %d", r.done, r.total, allocator = allocator)
}

// "Exported 10 of 12 · cancelled", then "2 failed" (none without failures).
outcome_words :: proc(r: Run, allocator := context.allocator) -> (words: string, failed: Maybe(string)) {
	words = fmt.aprintf("Exported %d of %d%s", r.done, r.total, " · cancelled" if r.cancelled else "", allocator = allocator)
	if r.failed > 0 {
		failed = fmt.aprintf("%d failed", r.failed, allocator = allocator)
	}
	return
}

// The status once it ends; `destination` is the batch's folder.
finished_words :: proc(r: Run, destination: string, allocator := context.allocator, style := fs.HOST) -> string {
	folder := ""
	if name, ok := fs.file_name(destination, style); ok {
		folder = fs.display(name, style, context.temp_allocator)
	}
	b := strings.builder_make(0, 96, context.temp_allocator)
	if r.done == 1 {
		strings.write_string(&b, "Exported 1 photograph")
	} else {
		fmt.sbprintf(&b, "Exported %d photographs", r.done)
	}
	fmt.sbprintf(&b, " to “%s”", folder)
	if r.failed > 0 {
		fmt.sbprintf(&b, "; %d failed", r.failed)
	}
	if r.skipped > 0 {
		fmt.sbprintf(&b, "; %d skipped", r.skipped)
	}
	if r.cancelled {
		return strings.concatenate({"Export cancelled. ", strings.to_string(b)}, allocator)
	}
	return strings.clone(strings.to_string(b), allocator)
}

// Lightroom's name by the key a batch keeps.
lightroom_title :: proc "contextless" (key: string) -> string {
	return "Lightroom Classic" if key == "classic" else "Lightroom"
}

// "Exporting 5 of 12 photographs", or "Sending 1 of 2 photographs to
// Lightroom" (`lightroom` is Lightroom's title).
exporting :: proc(done: u64, total: u64, lightroom: Maybe(string), allocator := context.allocator) -> string {
	to, sending := lightroom.?
	what := "photograph" if total == 1 else "photographs"
	return fmt.aprintf(
		"%s %d of %d %s%s%s",
		"Sending" if sending else "Exporting",
		min(done + 1, total),
		total,
		what,
		" to " if sending else "",
		to if sending else "",
		allocator = allocator,
	)
}

// The status line's and the grid foot's words while it runs.
progress_words :: proc(r: Run, lightroom_key: Maybe(string), allocator := context.allocator) -> string {
	title: Maybe(string)
	if key, ok := lightroom_key.?; ok {
		title = lightroom_title(key)
	}
	return exporting(finished(r), r.total, title, allocator)
}

// `ExportSize::summary`: none at full size.
size_summary :: proc(edge: Edge, pixels: u32, allocator := context.allocator) -> Maybe(string) {
	if edge == .Full {
		return nil
	}
	return fmt.aprintf("Fits %d px on the %s edge", pixels, "long" if edge == .Long else "short", allocator = allocator)
}

// `SaveSettings::summary(export, preview_space)`.
save_summary :: proc(save: Save, export: bool, preview_space: string, allocator := context.allocator) -> Maybe(string) {
	space := COLOR_SPACES[min(save.color_space, u64(len(COLOR_SPACES) - 1))]
	srgb := save.color_space == 0
	follows := "" if space == preview_space else fmt.tprintf("; the preview then shows %s", space)
	if export {
		if !srgb || follows != "" {
			return fmt.aprintf("Develops in %s and embeds its profile%s", space, follows, allocator = allocator)
		}
		return nil
	}
	if space == preview_space {
		if !srgb {
			return fmt.aprintf("Keeps the preview's %s colours and embeds its profile", space, allocator = allocator)
		}
		return nil
	}
	return fmt.aprintf(
		"Converts the %s preview to %s, clipping colours outside the smaller space%s",
		preview_space,
		space,
		follows,
		allocator = allocator,
	)
}

// The dialog's message: what the batch does, what the engine does, then
// the size and colour.
message :: proc(count: u64, engine: Export_Engine, edge: Edge, pixels: u32, save: Save, preview_space: string, allocator := context.allocator) -> string {
	b := strings.builder_make(0, 256, context.temp_allocator)
	if count == 1 {
		strings.write_string(&b, "With its own edits, crop and border.")
	} else {
		fmt.sbprintf(&b, "Each of the %d photographs with its own edits, crop and border, one after another.", count)
	}
	strings.write_byte(&b, ' ')
	switch engine {
	case .Fast:
		strings.write_string(&b, "Fast looks the same as Reference, in a fraction of the time.")
	case .Reference:
		strings.write_string(&b, "Reference is the most precise; about half a minute for each large photo.")
	}
	if s, ok := size_summary(edge, pixels, context.temp_allocator).?; ok {
		fmt.sbprintf(&b, " %s.", s)
	}
	if s, ok := save_summary(save, true, preview_space, context.temp_allocator).?; ok {
		fmt.sbprintf(&b, " %s.", s)
	}
	return strings.clone(strings.to_string(b), allocator)
}
