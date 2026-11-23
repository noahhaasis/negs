// The edit state's C ABI (negs_core.h, "Phase 4 base: value, edit state").
// A state crosses as the JSON serde_json writes for it; `negs_state_t` is
// one parsed state in its own arena, for callers asking several questions
// of it.
package negs_core

import "base:runtime"
import "core:strings"
import crop "../rules/crop"
import "../state"

State_Handle :: struct {
	arena: runtime.Arena,
	state: state.Edit_State,
}

@(private)
handle_new :: proc() -> ^State_Handle {
	h := new(State_Handle)
	_ = runtime.arena_init(&h.arena, 16 * 1024, context.allocator)
	return h
}

@(private)
handle_free :: proc(h: ^State_Handle) {
	runtime.arena_destroy(&h.arena)
	free(h)
}

@(export, link_name = "negs_state_parse")
state_parse :: proc "c" (text: [^]u8, n: uint) -> ^State_Handle {
	context = core_context()
	h := handle_new()
	s, err := state.parse(host_bytes(text, n), runtime.arena_allocator(&h.arena))
	if err != .None {
		handle_free(h)
		return nil
	}
	h.state = s
	return h
}

@(export, link_name = "negs_state_default")
state_default :: proc "c" () -> ^State_Handle {
	context = core_context()
	h := handle_new()
	h.state = state.default_state(runtime.arena_allocator(&h.arena))
	return h
}

@(export, link_name = "negs_state_free")
state_free :: proc "c" (h: ^State_Handle) {
	context = core_context()
	if h != nil {
		handle_free(h)
	}
}

@(export, link_name = "negs_state_write")
state_write :: proc "c" (h: ^State_Handle, flags: u32) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if h == nil {
		return {}
	}
	b := strings.builder_make(0, 1024, context.temp_allocator)
	{
		context.allocator = context.temp_allocator
		state.write(&b, h.state, flags & VALUE_SORTED != 0, flags & VALUE_PRETTY != 0)
	}
	return buf_from(b.buf[:])
}

@(export, link_name = "negs_state_eq")
state_eq :: proc "c" (a, b: ^State_Handle) -> u8 {
	context = core_context()
	if a == nil || b == nil {
		return u8(a == b)
	}
	return u8(state.equal(a.state, b.state))
}

@(export, link_name = "negs_state_rendered")
state_rendered :: proc "c" (h: ^State_Handle) -> ^State_Handle {
	context = core_context()
	if h == nil {
		return nil
	}
	out := handle_new()
	out.state = state.rendered(h.state, runtime.arena_allocator(&out.arena))
	return out
}

@(export, link_name = "negs_state_match_inputs")
state_match_inputs :: proc "c" (h: ^State_Handle) -> Buf {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if h == nil {
		return {}
	}
	return value_buf(state.match_inputs(h.state, context.temp_allocator))
}

@(export, link_name = "negs_state_rotation")
state_rotation :: proc "c" (h: ^State_Handle) -> i32 {
	context = core_context()
	return state.rotation(h.state) if h != nil else 0
}

@(export, link_name = "negs_state_flip")
state_flip :: proc "c" (h: ^State_Handle) -> u8 {
	context = core_context()
	return u8(state.flip(h.state)) if h != nil else 0
}

@(export, link_name = "negs_state_orientation")
state_orientation :: proc "c" (h: ^State_Handle, out: ^crop.Orientation) {
	context = core_context()
	if h != nil && out != nil {
		out^ = state.orientation(h.state)
	}
}

@(export, link_name = "negs_state_size_target")
state_size_target :: proc "c" (h: ^State_Handle, out: ^u32) -> u8 {
	context = core_context()
	if h == nil {
		return 0
	}
	t, ok := state.size_target(h.state.params).?
	if ok && out != nil {
		out^ = t
	}
	return u8(ok)
}

@(export, link_name = "negs_state_roll_filtered")
state_roll_filtered :: proc "c" (h: ^State_Handle, magenta, yellow: f64) -> u8 {
	context = core_context()
	if h == nil {
		return 0
	}
	return u8(state.roll_filtered(state.Roll{magenta = magenta, yellow = yellow}, h.state))
}

@(export, link_name = "negs_state_roll_own_filtration")
state_roll_own_filtration :: proc "c" (roll_paper: [^]u8, n: uint, has_paper: u8, paper: [^]u8, paper_len: uint) -> cstring {
	context = core_context()
	r: state.Roll
	if has_paper != 0 {
		r.paper = host_string(roll_paper, n)
	}
	return own_filtration_cstring(state.roll_own_filtration(r, host_string(paper, paper_len)))
}

@(private)
own_filtration_cstring :: proc "contextless" (s: string) -> cstring {
	return state.ROLL_OTHER_PAPER if s == state.ROLL_OTHER_PAPER else state.ROLL_OWN
}

// MARK: On one value's JSON (the value a path holds), without a state

@(export, link_name = "negs_state_rotation_of")
state_rotation_of :: proc "c" (v: [^]u8, n: uint) -> i32 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	tree, _ := host_value(v, n)
	return state.rotation_of(tree)
}

@(export, link_name = "negs_state_flip_of")
state_flip_of :: proc "c" (v: [^]u8, n: uint) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	tree, _ := host_value(v, n)
	return u8(state.flip_of(tree))
}

@(export, link_name = "negs_state_size_target_of")
state_size_target_of :: proc "c" (v: [^]u8, n: uint, out: ^u32) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	tree, _ := host_value(v, n)
	t, ok := state.size_target_of(tree).?
	if ok && out != nil {
		out^ = t
	}
	return u8(ok)
}

@(export, link_name = "negs_state_params_size_target")
state_params_size_target :: proc "c" (params: [^]u8, n: uint, out: ^u32) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	tree, _ := host_value(params, n)
	t, ok := state.size_target(tree).?
	if ok && out != nil {
		out^ = t
	}
	return u8(ok)
}

@(export, link_name = "negs_state_roll_filtered_of")
state_roll_filtered_of :: proc "c" (roll_magenta, roll_yellow: f64, magenta: [^]u8, m_len: uint, yellow: [^]u8, y_len: uint) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	m, _ := host_value(magenta, m_len)
	y, _ := host_value(yellow, y_len)
	return u8(state.roll_filtered_of(roll_magenta, roll_yellow, m, y))
}

@(export, link_name = "negs_state_restored_params")
state_restored_params :: proc "c" (defaults: [^]u8, d_len: uint, saved: [^]u8, s_len: uint, nullable: [^]Str_C, count: uint, out: ^Buf) -> u8 {
	context = core_context()
	arena: runtime.Arena
	context.temp_allocator = runtime.arena_allocator(call_begin(&arena))
	if out != nil {
		out^ = {}
	}
	d, ok_d := host_value(defaults, d_len)
	s, ok_s := host_value(saved, s_len)
	if !ok_d || !ok_s {
		return 0
	}
	r := state.restored_params(d, s, host_strings(nullable, count), context.temp_allocator)
	store_buf(out, value_buf(r))
	return 1
}

// state.rs's path constants and the default film and paper: 0
// PREP_EXPOSURE, 1 FLIP, 2 SIZE_TARGET, 3 SIZE_FACTOR, 4 LENS_CORRECTION,
// 5 the default film, 6 the default paper, 7 Roll's word for another
// paper, 8 its word otherwise; NULL past them.
@(export, link_name = "negs_state_constant")
state_constant :: proc "c" (which: u32) -> cstring {
	switch which {
	case 0:
		return state.PREP_EXPOSURE
	case 1:
		return state.FLIP
	case 2:
		return state.SIZE_TARGET
	case 3:
		return state.SIZE_FACTOR
	case 4:
		return state.LENS_CORRECTION
	case 5:
		return state.DEFAULT_FILM
	case 6:
		return state.DEFAULT_PAPER
	case 7:
		return state.ROLL_OTHER_PAPER
	case 8:
		return state.ROLL_OWN
	}
	return nil
}
