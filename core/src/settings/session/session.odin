// session.rs's EditorSession and SessionStore: the editing session saved
// at quit, `session.json` in the settings folder, read and written as its
// serde derive and serde_json do (`serde_json::to_vec`, compact).
//
// Everything here lives in the context's allocator and the temporary one:
// callers run in an arena (the ABI's per-call one).
package session

import "core:math"
import "core:strings"
import "../files"
import "../paths"
import "../../rules/crop"
import "../../state"
import "../../value"

// 2: the DIR gammas and halation baseline are null unless set, and a set
// value beats the film's preset. Schema 1 stored values for them that the
// engine ignored and nothing could set, so `migrate` drops them.
SCHEMA :: 2

// state.rs's DIR_HALATION_STOCK_PATHS (`Stock.dirHalationPaths`).
DIR_HALATION_STOCK_PATHS := [?]string {
	"film_render.dir_couplers.gamma_samelayer_rgb",
	"film_render.dir_couplers.gamma_interlayer_r_to_gb",
	"film_render.dir_couplers.gamma_interlayer_g_to_rb",
	"film_render.dir_couplers.gamma_interlayer_b_to_rg",
	"film_render.halation.halation_strength",
	"film_render.halation.halation_first_sigma_um",
}

// The largest session.json restored.
MAX_SESSION_SIZE :: 1_048_576

Editor_Session :: struct {
	schema:            u32,
	// The photograph's path. There is no bookmark on Windows.
	source:            string,
	state:             state.Edit_State,
	inspector_visible: bool,
	fit_image:         bool,
	library:           bool,
}

SESSION_FIELDS := [?]string{"schema", "source", "state", "inspectorVisible", "fitImage", "library"}

read_session :: proc(p: ^Parser) -> (out: Editor_Session, ok: bool) {
	out.inspector_visible = true
	out.fit_image = true
	field :: proc(p: ^Parser, index: int, data: rawptr) -> bool {
		s := (^Editor_Session)(data)
		switch index {
		case 0:
			s.schema = read_u32(p) or_return
		case 1:
			s.source = read_string(p) or_return
		case 2:
			s.state = state.read_state(p) or_return
		case 3:
			s.inspector_visible = read_bool(p) or_return
		case 4:
			s.fit_image = read_bool(p) or_return
		case 5:
			s.library = read_bool(p) or_return
		}
		return true
	}
	seen := read_struct(p, SESSION_FIELDS[:], &out, field) or_return
	if !({0, 1, 2} <= seen) {
		return out, value.fail(p, .Missing_Field)
	}
	return out, true
}

// `serde_json::from_slice::<EditorSession>`.
parse_session :: proc(text: []u8) -> (Editor_Session, bool) {
	p := value.parser_make(text, context.allocator)
	s, ok := read_session(&p)
	return s, ok && value.finish(&p)
}

// `serde_json::to_vec(&session)`.
write_session :: proc(b: ^strings.Builder, s: Editor_Session) {
	strings.write_string(b, `{"schema":`)
	strings.write_u64(b, u64(s.schema))
	strings.write_string(b, `,"source":`)
	value.write_string(b, s.source)
	strings.write_string(b, `,"state":`)
	state.write(b, s.state)
	strings.write_string(b, `,"inspectorVisible":`)
	strings.write_string(b, "true" if s.inspector_visible else "false")
	strings.write_string(b, `,"fitImage":`)
	strings.write_string(b, "true" if s.fit_image else "false")
	strings.write_string(b, `,"library":`)
	strings.write_string(b, "true" if s.library else "false")
	strings.write_byte(b, '}')
}

session_text :: proc(s: Editor_Session) -> []u8 {
	b := strings.builder_make(0, 4096, context.allocator)
	write_session(&b, s)
	return b.buf[:]
}

// Schema 1's DIR and halation values become null (as json::set makes them)
// and the schema the current one.
migrate :: proc(s: ^Editor_Session) {
	if s.schema != 1 {
		return
	}
	migrate_params(&s.state.params)
	s.schema = SCHEMA
}

migrate_params :: proc(params: ^value.Value) {
	for path in DIR_HALATION_STOCK_PATHS {
		value.set(params, path, nil)
	}
}

// `EditorSession::validate` without its message: the schema, an absolute
// source, a crop inside the photograph and a quarter-turn rotation.
validate :: proc(s: Editor_Session) -> bool {
	c := s.state.crop
	values := [?]f64{c.rect.x, c.rect.y, c.rect.width, c.rect.height, c.custom.width, c.custom.height, c.angle}
	finite := true
	for v in values {
		if v - v != 0 {
			finite = false
		}
	}
	rotation := -1.0
	if n, is_number := value.as_f64(value.get(s.state.params, state.ROTATE)); is_number {
		rotation = n
	}
	_, is_object := s.state.params.(value.Object)
	return(
		s.schema == SCHEMA &&
		paths.is_absolute(s.source, paths.HOST) &&
		finite &&
		c.rect.x >= 0 &&
		c.rect.y >= 0 &&
		c.rect.width > 0 &&
		c.rect.height > 0 &&
		c.rect.x + c.rect.width <= 1.000001 &&
		c.rect.y + c.rect.height <= 1.000001 &&
		abs(c.angle) <= crop.MAX_ANGLE &&
		c.custom.width > 0 &&
		c.custom.height > 0 &&
		is_object &&
		rotation >= 0 &&
		rotation <= 3 &&
		math.floor(rotation) == rotation \
	)
}

INVALID_SESSION :: "The previous editing session could not be restored."

Load_Result :: enum u8 {
	// `text` is the error.
	Error       = 0,
	None        = 1,
	// `text` is the session as `write_session` writes it.
	Session     = 2,
	// The file isn't a session: `text` is its bytes, for serde's message.
	Unreadable  = 3,
}

// `SessionStore::load`.
load :: proc(file: string) -> (Load_Result, []u8) {
	st, code := files.stat(file)
	if code != 0 {
		return .None, nil
	}
	if st.size > MAX_SESSION_SIZE {
		return .Error, transmute([]u8)string("The previous editing session is too large to restore.")
	}
	data: []u8
	data, code = files.read_file(file, context.allocator)
	if code != 0 {
		return .Error, transmute([]u8)files.error_text(code, context.allocator)
	}
	s, ok := parse_session(data)
	if !ok {
		return .Unreadable, data
	}
	migrate(&s)
	if !validate(s) {
		return .Error, transmute([]u8)string(INVALID_SESSION)
	}
	source, sc := files.stat(s.source)
	if sc != 0 || !source.is_file {
		return .Error, transmute([]u8)string("The previous photograph is unavailable. Open a photograph to continue.")
	}
	return .Session, session_text(s)
}

Save_Result :: enum u8 {
	// `text` is the error.
	Error      = 0,
	Saved      = 1,
	// The text isn't a session the core reads (a double serde wrote as
	// null): nothing was done.
	Unreadable = 2,
}

// `SessionStore::save` of the session serde_json wrote as `text`.
save :: proc(file: string, text: []u8) -> (Save_Result, string) {
	s, ok := parse_session(text)
	if !ok {
		return .Unreadable, ""
	}
	if !validate(s) {
		return .Error, INVALID_SESSION
	}
	if e, fine := files.write_atomic(file, session_text(s), context.allocator); !fine {
		return .Error, e
	}
	return .Saved, ""
}
