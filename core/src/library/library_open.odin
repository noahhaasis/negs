// The open photograph: opening, editing and saving it, and what its
// sidecar holds for opening and for thumbnails and exports (library.rs).
package library

import "core:fmt"
import "core:strings"

// What opening a photograph needs from its sidecar (`SidecarRestore`),
// temporary.
@(private)
Restore :: struct {
	exists:           bool,
	variant:          string,
	new_variant:      bool,
	defaults_version: i64,
	rating:           u8,
	flag:             Flag,
	has_edits:        bool,
	edits:            Saved_Edits,
	has_problem:      bool,
	problem:          string,
}

// The variant an item opens: its own, or the file's first.
@(private)
chosen_variant :: proc(s: Sidecar, item: Item) -> (Variant, bool) {
	if item.has_variant {
		if v, ok := sidecar_variant(s, item.variant); ok {
			return v, true
		}
	}
	if len(s.variants) > 0 {
		return s.variants[0], true
	}
	return {}, false
}

// The film shown for one this version lacks: the table's, or the first.
@(private)
fallback_film :: proc(d: Edit_Defaults) -> string {
	if _, ok := film_lookup(d, d.table_film); ok {
		return d.table_film
	}
	return d.films[0].name if len(d.films) > 0 else ""
}

@(private)
fallback_paper :: proc(d: Edit_Defaults) -> string {
	if paper_known(d, d.table_paper) {
		return d.table_paper
	}
	return d.papers[0] if len(d.papers) > 0 else ""
}

@(private)
film_missing :: proc(d: Edit_Defaults, film: string) -> bool {
	_, ok := film_lookup(d, film)
	return len(d.films) > 0 && !ok
}

@(private)
paper_missing :: proc(d: Edit_Defaults, paper: string) -> bool {
	return len(d.papers) > 0 && !paper_known(d, paper)
}

// The open photograph's item, if one is (the library's own).
library_open_item :: proc(lib: ^Library) -> (Item, bool) {
	return lib.open.item, lib.has_open
}

// What the open photograph shows (the library's own).
library_state :: proc(lib: ^Library) -> (Edit_State, bool) {
	return lib.open.state, lib.has_open
}

// The open photograph has edits its sidecar keeps.
library_open_edited :: proc(lib: ^Library) -> bool {
	if !lib.has_open {
		return false
	}
	o := &lib.open
	return o.edited_at_open || !edit_state_equal(user_edits(o.state, context.temp_allocator), o.opened)
}

// What `item`'s developed thumbnail is made from, read from its sidecar now
// (temporary): false without stored edits or with a sidecar that can't be
// read.
library_thumbnail_state :: proc(lib: ^Library, item: Item) -> (state: Edit_State, match_solved: bool, ok: bool) {
	if !lib.sidecars {
		return
	}
	ta := context.temp_allocator
	s, found, problem := sidecar_read(item.photo)
	if problem != nil || !found {
		return
	}
	chosen := chosen_variant(s, item) or_return
	table, _ := tables_get(&lib.tables, s.defaults_version)
	if table == nil || chosen.edits == nil {
		return
	}
	saved, bad := decode_edits(chosen.edits, table)
	if bad != nil {
		return
	}
	d := lib.defaults
	state = restored_state(d, saved.state, ta)
	if film_missing(d, state.film) {
		shown := fallback_film(d)
		positive, _ := film_lookup(d, shown)
		json_set_at(&state.params, "io.scan_film", positive, ta)
		state.film = strings.clone(shown, ta)
	}
	if paper_missing(d, state.paper) {
		state.paper = strings.clone(fallback_paper(d), ta)
	}
	return state, saved.match_solved, true
}

// What a batch export develops for `item` (temporary).
library_export_edits :: proc(lib: ^Library, item: Item) -> (Export_Edits, bool) {
	if !lib.sidecars {
		return {}, false
	}
	ta := context.temp_allocator
	s, found, problem := sidecar_read(item.photo)
	if problem != nil || !found {
		return {}, false
	}
	chosen, has_chosen := chosen_variant(s, item)
	if !has_chosen || chosen.edits == nil {
		return {}, false
	}
	table, _ := tables_get(&lib.tables, s.defaults_version)
	if table == nil {
		return {}, false
	}
	saved, bad := decode_edits(chosen.edits, table)
	if bad != nil {
		return {}, false
	}
	d := lib.defaults
	state := restored_state(d, saved.state, ta)
	missing := make([dynamic]string, 0, 2, ta)
	if film_missing(d, state.film) {
		append(&missing, fmt.aprintf("the film “%s”", state.film, allocator = ta))
	}
	if paper_missing(d, state.paper) {
		append(&missing, fmt.aprintf("the paper “%s”", state.paper, allocator = ta))
	}
	return Export_Edits{state, saved.match_solved, missing[:], look(chosen.edits, s.defaults_version, ta)}, true
}

// Stores a brightness match a batch solved for `item`'s stored edits, while
// they are still the ones it was solved for. True if the sidecar was
// written.
library_store_solved_match :: proc(lib: ^Library, item: Item, look_name: string, state: Edit_State) -> bool {
	ta := context.temp_allocator
	if !lib.sidecars || (lib.has_open && item_equal(lib.open.item, item)) || read_only_photo(item.photo) {
		return false
	}
	s, found, problem := sidecar_read(item.photo)
	if problem != nil || !found {
		return false
	}
	variant, has_variant := chosen_variant(s, item)
	if !has_variant || variant.edits == nil || look(variant.edits, s.defaults_version, ta) != look_name {
		return false
	}
	m := state.brightness_match
	if !(state.has_match && m.has_ev && m.solved) {
		return false
	}
	table, _ := tables_get(&lib.tables, s.defaults_version)
	if table == nil {
		return false
	}
	patch := patch_new(variant.id, s.defaults_version, lib.app)
	patch.primary = !item.has_variant
	patch.set_edits = true
	patch.edits = encode_edits(state, true, table, ta)
	patch.known = defaults_table_known(table, lib.defaults.params, true)
	if _, ok := write(lib, item.photo, &patch); !ok {
		return false
	}
	reload(lib, item.photo)
	return true
}

// How a photograph without stored edits develops for its thumbnail
// (temporary).
library_unedited_state :: proc(lib: ^Library) -> Edit_State {
	return new_photo_state(lib.defaults, edit_state_empty(context.temp_allocator), context.temp_allocator)
}

// Opens `item`: the photograph open before is saved first, and remembered
// for Paste from Previous if it has edits (temporary).
library_open :: proc(lib: ^Library, item_in: Item) -> Opened {
	ta := context.temp_allocator
	item := item_clone(item_in, ta)
	previous := edit_state_empty(ta)
	if lib.has_open {
		library_save(lib)
		if lib.open.written != nil {
			set_previous(lib, user_edits(lib.open.state, ta))
		}
		previous = edit_state_clone(lib.open.state, ta)
	}
	r := restore(lib, item.photo, item)
	return adopt(lib, item, r, previous)
}

// Saves the open photograph and closes it, as at quit.
library_close :: proc(lib: ^Library) -> (string, bool) {
	error, saved := library_save(lib)
	if lib.has_open {
		if lib.open.written != nil {
			set_previous(lib, user_edits(lib.open.state, context.temp_allocator))
		}
		close_open(lib)
	}
	return error, saved
}

// The user changed the open photograph: kept in memory until saved.
library_edit :: proc(lib: ^Library, state: Edit_State) {
	if lib.has_open {
		set_open_state(lib, state)
	}
}

// Whether `library_save` would write.
library_unsaved :: proc(lib: ^Library) -> bool {
	if !lib.has_open || lib.open.has_blocked {
		return false
	}
	_, send := edits_to_send(lib, &lib.open)
	return send
}

// Writes the open photograph's edits if they changed. The error says why it
// couldn't, once per folder (temporary).
library_save :: proc(lib: ^Library) -> (string, bool) {
	if !lib.sidecars || !lib.has_open || lib.open.has_blocked {
		return "", true
	}
	ta := context.temp_allocator
	edits, send := edits_to_send(lib, &lib.open)
	if !send {
		return "", true
	}
	open := &lib.open
	table, err := tables_get(&lib.tables, open.defaults_version)
	if table == nil {
		return err, false
	}
	patch := patch_new(strings.clone(open.variant, ta), open.defaults_version, lib.app)
	patch.primary = open.variant_is_new || !open.item.has_variant
	patch.set_edits = true
	patch.edits = edits
	patch.known = defaults_table_known(table, lib.defaults.params, true)
	photo := strings.clone(open.item.photo, ta)
	if reason, ok := write(lib, photo, &patch); !ok {
		outcome := Outcome{}
		failed_write(lib, photo, reason, &outcome)
		return outcome.error, false
	}
	reload(lib, photo)
	set_open_written(lib, edits)
	return "", true
}

// The edits the open photograph's sidecar would be sent now (nil removes
// them); false when nothing changed (`sidecarEditsToSend`).
@(private)
edits_to_send :: proc(lib: ^Library, open: ^Open) -> (Json, bool) {
	edited := !edit_state_equal(user_edits(open.state, context.temp_allocator), open.opened)
	edits: Json
	if edited || open.edited_at_open {
		stored, ok := stored_edits(lib, open.state, open)
		if !ok {
			return nil, false
		}
		edits = stored
	}
	return edits, !json_equal(edits, open.written)
}

// `state` as the open photograph's sidecar stores it (temporary).
@(private)
stored_edits :: proc(lib: ^Library, state: Edit_State, open: ^Open) -> (Json, bool) {
	table, _ := tables_get(&lib.tables, open.defaults_version)
	if table == nil {
		return nil, false
	}
	saved := state
	if open.has_missing_film && saved.film == open.missing_film.shown {
		saved.film = open.missing_film.name
	}
	if open.has_missing_paper && saved.paper == open.missing_paper.shown {
		saved.paper = open.missing_paper.name
	}
	m := state.brightness_match
	solved := state.has_match && m.has_ev && m.solved
	return encode_edits(saved, solved, table, context.temp_allocator), true
}

// Reads `photo`'s sidecar for opening `item`'s variant (or its first).
@(private)
restore :: proc(lib: ^Library, photo: string, item: Item) -> Restore {
	ta := context.temp_allocator
	r := Restore {
		variant          = next_id(lib),
		new_variant      = true,
		defaults_version = LATEST_DEFAULTS,
	}
	if !lib.sidecars {
		return r
	}
	s, found, problem := sidecar_read(photo)
	if p, bad := problem.?; bad {
		r.exists = true
		r.has_problem = true
		r.problem = problem_message(p, file_name(photo), ta)
		return r
	}
	if !found {
		return r
	}
	r.exists = true
	r.defaults_version = s.defaults_version
	chosen, has_chosen := chosen_variant(s, item)
	if !has_chosen {
		return r
	}
	r.variant = strings.clone(chosen.id, ta)
	r.new_variant = false
	r.rating = chosen.rating
	r.flag = chosen.flag
	if chosen.edits != nil {
		table, err := tables_get(&lib.tables, s.defaults_version)
		if table == nil {
			r.has_problem = true
			r.problem = problem_message(unreadable(err), file_name(photo), ta)
			return r
		}
		saved, bad := decode_edits(chosen.edits, table)
		if p, is_bad := bad.?; is_bad {
			r.has_problem = true
			r.problem = problem_message(p, file_name(photo), ta)
		} else {
			r.has_edits = true
			r.edits = saved
		}
	}
	return r
}

// Sets the open photograph from what its sidecar held (`adoptEdits`).
// Returns what it opened with (temporary).
@(private)
adopt :: proc(lib: ^Library, item_in: Item, r: Restore, previous: Edit_State) -> Opened {
	ta := context.temp_allocator
	a := lib.allocator
	item := item_clone(item_in, ta)
	opened := Opened{has_problem = r.has_problem, problem = r.problem}
	missing_film, missing_paper: Missing
	has_missing_film, has_missing_paper: bool
	state: Edit_State
	d := lib.defaults
	if r.has_edits && !r.has_problem {
		restored := restored_state(d, r.edits.state, ta)
		// A film or paper this version lacks falls back to the table's.
		if film_missing(d, restored.film) {
			shown := strings.clone(fallback_film(d), ta)
			has_missing_film = true
			missing_film = Missing{restored.film, shown}
			positive, _ := film_lookup(d, shown)
			json_set_at(&restored.params, "io.scan_film", positive, ta)
			restored.film = shown
		}
		if paper_missing(d, restored.paper) {
			shown := strings.clone(fallback_paper(d), ta)
			has_missing_paper = true
			missing_paper = Missing{restored.paper, shown}
			restored.paper = shown
		}
		if has_missing_film || has_missing_paper {
			names := make([dynamic]string, 0, 2, ta)
			shown := make([dynamic]string, 0, 2, ta)
			if has_missing_film {
				append(&names, fmt.aprintf("the film “%s”", missing_film.name, allocator = ta))
				append(&shown, missing_film.shown)
			}
			if has_missing_paper {
				append(&names, fmt.aprintf("the paper “%s”", missing_paper.name, allocator = ta))
				append(&shown, missing_paper.shown)
			}
			opened.has_problem = true
			opened.problem = fmt.aprintf(
				"%s was edited with %s, which this version of Negs doesn’t have. It shows %s instead, and keeps the saved choice unless you pick another.",
				file_name(item.photo),
				strings.join(names[:], " and ", ta),
				strings.join(shown[:], " and ", ta),
				allocator = ta,
			)
		}
		state = restored
	} else {
		state = new_photo_state(d, previous, ta)
	}
	if item_key(item, ta) not_in lib.summaries {
		set_summary(lib, item, Summary{rating = r.rating, flag = r.flag, edited = r.has_edits, has_problem = r.has_problem, problem = r.problem})
	}
	next := Open {
		item              = item,
		variant           = r.variant,
		variant_is_new    = r.new_variant,
		defaults_version  = r.defaults_version,
		opened            = user_edits(state, ta),
		edited_at_open    = r.has_edits,
		has_blocked       = r.has_problem,
		blocked           = r.problem,
		has_missing_film  = has_missing_film,
		missing_film      = missing_film,
		has_missing_paper = has_missing_paper,
		missing_paper     = missing_paper,
		state             = state,
	}
	if next.edited_at_open && !next.has_blocked {
		next.written, _ = stored_edits(lib, state, &next)
	}
	close_open(lib)
	lib.open = open_clone(next, a)
	lib.has_open = true
	opened.state = state
	return opened
}

// Another app changed the open photograph's sidecar. Newer edits replace
// these only if they're all saved (`sidecarChangedOnDisk`).
@(private)
sidecar_changed_on_disk :: proc(lib: ^Library, interacting: bool) -> Outcome {
	ta := context.temp_allocator
	outcome := Outcome{}
	if !lib.has_open {
		return outcome
	}
	open := open_clone(lib.open, ta)
	if open.has_blocked {
		return outcome
	}
	r := restore(lib, open.item.photo, open.item)
	if r.has_problem {
		// Never written over now.
		set_blocked(lib, r.problem)
		return outcome
	}
	if !r.new_variant && lib.open.variant_is_new {
		set_string(lib, &lib.open.variant, r.variant)
		lib.open.variant_is_new = false
	}
	has_ours := false
	ours: Saved_Edits
	if open.written != nil {
		if table, _ := tables_get(&lib.tables, open.defaults_version); table != nil {
			saved, bad := decode_edits(open.written, table)
			if bad == nil {
				has_ours, ours = true, saved
			}
		}
	}
	now := open_clone(lib.open, ta)
	same := r.has_edits == has_ours && (!has_ours || (edit_state_equal(r.edits.state, ours.state) && r.edits.match_solved == ours.match_solved))
	_, unsent := edits_to_send(lib, &now)
	if (same && r.defaults_version == now.defaults_version) || interacting || unsent {
		return outcome
	}
	if now.item.has_variant && r.new_variant {
		// Its duplicate is gone: its photograph opens instead.
		close_open(lib)
		outcome.open_gone = true
		outcome.has_open = true
		outcome.open = item_photo(now.item.photo)
		return outcome
	}
	adopt(lib, now.item, r, now.state)
	outcome.open_changed = true
	return outcome
}

// `item`'s edits as `user_edits` gives them, and whether it has any of its
// own; false if its sidecar can't be read (temporary).
@(private)
user_edits_of :: proc(lib: ^Library, item: Item) -> (state: Edit_State, edited: bool, ok: bool) {
	ta := context.temp_allocator
	if lib.has_open && item_equal(lib.open.item, item) {
		return user_edits(lib.open.state, ta), library_open_edited(lib), true
	}
	if !lib.sidecars {
		return user_edits(plain_state(lib.defaults, ta), ta), false, true
	}
	r := restore(lib, item.photo, item)
	if r.has_problem {
		return
	}
	if !r.has_edits {
		return user_edits(plain_state(lib.defaults, ta), ta), false, true
	}
	return user_edits(restored_state(lib.defaults, r.edits.state, ta), ta), true, true
}

// `item`'s edits as it opens with them (`openingState`); false if its
// sidecar can't be read (temporary).
library_opening_state :: proc(lib: ^Library, item: Item) -> (Edit_State, bool) {
	ta := context.temp_allocator
	if lib.has_open && item_equal(lib.open.item, item) {
		return edit_state_clone(lib.open.state, ta), true
	}
	if !lib.sidecars {
		return plain_state(lib.defaults, ta), true
	}
	r := restore(lib, item.photo, item)
	if r.has_problem {
		return {}, false
	}
	if !r.has_edits {
		return plain_state(lib.defaults, ta), true
	}
	return restored_state(lib.defaults, r.edits.state, ta), true
}
