// The context every exported procedure runs in: the default one, with the
// heap wrapped in a mutex-guarded mem.Tracking_Allocator when the library is
// built with -define:NEGS_CORE_TRACK=true (`build.py lib --track`), so the
// hosts' differential tests can assert that nothing leaks across the ABI.
package negs_core

import "base:runtime"
import "core:mem"
import "core:sync"

NEGS_CORE_TRACK :: #config(NEGS_CORE_TRACK, false)

@(private)
Tracker :: struct {
	once:     sync.Once,
	mutex:    sync.Mutex,
	tracking: mem.Tracking_Allocator,
	total:    i64,
}

@(private)
tracker: Tracker

@(private)
tracked_proc :: proc(
	data: rawptr,
	mode: mem.Allocator_Mode,
	size, alignment: int,
	old_memory: rawptr,
	old_size: int,
	location := #caller_location,
) -> (
	[]byte,
	mem.Allocator_Error,
) {
	t := (^Tracker)(data)
	sync.mutex_guard(&t.mutex)
	if mode == .Alloc || mode == .Alloc_Non_Zeroed {
		t.total += 1
	}
	return mem.tracking_allocator_proc(&t.tracking, mode, size, alignment, old_memory, old_size, location)
}

// A static library (or, on Windows, an object) has no entry point, so
// nothing runs Odin's runtime startup: globals that need code to
// initialise (core:unicode's range tables, which is_letter and is_upper
// read for anything past Latin-1) would stay zero. The first call into the
// core runs it.
@(private)
started: sync.Once

// The context for an exported procedure's body: `context = core_context()`.
core_context :: proc "contextless" () -> runtime.Context {
	c := runtime.default_context()
	when ODIN_BUILD_MODE == .Static || ODIN_BUILD_MODE == .Object {
		context = c
		sync.once_do(&started, proc() {
			runtime._startup_runtime()
		})
	}
	when NEGS_CORE_TRACK {
		context = c
		sync.once_do(&tracker.once, proc() {
			mem.tracking_allocator_init(&tracker.tracking, runtime.heap_allocator(), runtime.heap_allocator())
			tracker.tracking.bad_free_callback = mem.tracking_allocator_bad_free_callback_add_to_array
		})
		c.allocator = mem.Allocator{tracked_proc, &tracker}
	}
	return c
}

Alloc_Stats :: struct {
	live_bytes:  i64,
	live_count:  i64,
	total_count: i64,
	bad_frees:   i64,
	tracked:     u8,
}

@(export, link_name = "negs_core_alloc_stats")
alloc_stats :: proc "c" (out: ^Alloc_Stats) {
	if out == nil {
		return
	}
	out^ = {}
	when NEGS_CORE_TRACK {
		context = core_context()
		sync.mutex_guard(&tracker.mutex)
		out.live_bytes = tracker.tracking.current_memory_allocated
		out.live_count = i64(len(tracker.tracking.allocation_map))
		out.total_count = tracker.total
		out.bad_frees = i64(len(tracker.tracking.bad_free_array))
		out.tracked = 1
	}
}
