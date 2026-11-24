// core/src/settings/session: session.rs's tests that need no app, and the
// serde shapes of EditorSession and Preferences.
package session_tests

import "core:strings"
import "core:testing"
import "../../../src/settings/session"
import "../../../src/value"

SESSION :: `{"schema":2,"source":"/photos/DSC_1.NEF","state":{"params":{"io":{"rotate_cw":1}},"film":"kodak_gold_200","paper":"kodak_portra_endura"}}`

@(private)
parse :: proc(text: string) -> (session.Editor_Session, bool) {
	return session.parse_session(transmute([]u8)text)
}

@(test)
sessions_read_with_their_defaults :: proc(t: ^testing.T) {
	context.allocator = context.temp_allocator
	s, ok := parse(SESSION)
	testing.expect(t, ok)
	testing.expect(t, s.inspector_visible && s.fit_image && !s.library)
	testing.expect(t, session.validate(s))
	text := string(session.session_text(s))
	testing.expect(t, strings.has_prefix(text, `{"schema":2,"source":"/photos/DSC_1.NEF","state":{"params":{"io":{"rotate_cw":1}},"film"`))
	testing.expect(t, strings.has_suffix(text, `,"inspectorVisible":true,"fitImage":true,"library":false}`))
	_, ok = parse(`{"source":"/a","state":{"params":{},"film":"","paper":""}}`)
	testing.expect(t, !ok, "schema is required")
	_, ok = parse(`{"schema":2.0,"source":"/a","state":{"params":{},"film":"","paper":""}}`)
	testing.expect(t, !ok, "a double isn't a u32")
	_, ok = parse(`{"schema":2,"schema":2,"source":"/a","state":{"params":{},"film":"","paper":""}}`)
	testing.expect(t, !ok, "a field twice")
	s, ok = parse(`[2,"/a",{"params":{},"film":"","paper":""},false]`)
	testing.expect(t, ok && !s.inspector_visible && s.fit_image, "a struct can be an array")
}

@(test)
validation_rejects_bad_crops_and_rotations :: proc(t: ^testing.T) {
	context.allocator = context.temp_allocator
	s, _ := parse(SESSION)
	bad := s
	bad.state.crop.rect = {0.5, 0, 0.6, 1}
	testing.expect(t, !session.validate(bad))
	bad = s
	bad.state.crop.angle = 46
	testing.expect(t, !session.validate(bad))
	bad = s
	bad.source = "relative.png"
	testing.expect(t, !session.validate(bad))
	bad, _ = parse(`{"schema":2,"source":"/a","state":{"params":{"io":{"rotate_cw":1.5}},"film":"","paper":""}}`)
	testing.expect(t, !session.validate(bad))
}

@(test)
schema_1_drops_dir_and_halation_values :: proc(t: ^testing.T) {
	context.allocator = context.temp_allocator
	s, ok := parse(`{"schema":1,"source":"/a","state":{"params":{"io":{"rotate_cw":1},"film_render":{"dir_couplers":{"gamma_samelayer_rgb":[0.341,0.324,0.273],"amount":0.8},"halation":{"halation_strength":[0.05,0.015,0.0]}}},"film":"","paper":""}}`)
	testing.expect(t, ok)
	session.migrate(&s)
	testing.expect_value(t, s.schema, 2)
	testing.expect(t, value.get(s.state.params, "film_render.dir_couplers.gamma_samelayer_rgb") == nil)
	testing.expect(t, value.number(value.get(s.state.params, "film_render.dir_couplers.amount")) == 0.8)
	testing.expect(t, session.validate(s))
}

@(test)
preferences_take_their_defaults :: proc(t: ^testing.T) {
	context.allocator = context.temp_allocator
	old, ok := session.parse_preferences(transmute([]u8)string(`{"recentFiles": [], "checkForUpdates": false, "inspectorTab": "Post"}`))
	testing.expect(t, ok && old.previews && !old.check_for_updates && old.app_theme == "darkroom")
	testing.expect(t, session.tab(old.inspector_tab, old.inspector_tab_chosen) == .Output)
	testing.expect(t, session.tab("Lab", false) == .Prep && session.tab("Lab", true) == .Lab)
	testing.expect_value(t, len(old.favorite_films), 4)
	_, ok = session.parse_preferences(transmute([]u8)string(`[]`))
	testing.expect(t, !ok, "flatten reads a map only")
	_, ok = session.parse_preferences(transmute([]u8)string(`{"other": 1e400}`))
	testing.expect(t, !ok, "unknown values are read in full")
	_, ok = session.parse_preferences(transmute([]u8)string(`{"advanceAfterRating": null}`))
	testing.expect(t, !ok, "a flattened bool must be a bool")
	_, ok = session.parse_preferences(transmute([]u8)string(`{"sidecars": true, "sidecars": false}`))
	testing.expect(t, !ok)
	kept, kept_ok := session.parse_preferences(transmute([]u8)string(`{"keep_paper": true, "window": [1, 2, 3, 4, -5], "sidecars": false}`))
	testing.expect(t, kept_ok && kept.keep_paper && kept.window.value[4] == -5)
	text := string(session.preferences_text(kept, false))
	testing.expect(t, strings.contains(text, `"keep_paper":true`))
	testing.expect(t, strings.contains(text, `"appTheme":"","libraryFilter":null,`) == false)
	testing.expect(t, strings.contains(text, `"separateCameraJPEGs":false,"sidecars":false,"fotufilmFilms":"all"`))
	_, ok = session.parse_preferences(transmute([]u8)string(`{"window": [1, 2, 3, 4]}`))
	testing.expect(t, !ok)
	pretty := string(session.preferences_text(session.default_preferences()))
	testing.expect(t, strings.has_prefix(pretty, "{\n  \"recentFiles\": [],\n  \"filmstripVisible\": true,\n  \"gridSize\": 176.0,\n"))
}

@(test)
recent_groups_and_revealed_lists :: proc(t: ^testing.T) {
	context.allocator = context.temp_allocator
	testing.expect_value(t, session.list_set("Enlarger", "Grain", true, false), "Enlarger,Grain")
	testing.expect_value(t, session.list_set("Enlarger,Grain", "Enlarger", false, false), "Grain")
	testing.expect_value(t, session.list_set("b,,a", "c", true, true), "a,b,c")
	testing.expect(t, session.list_has("Enlarger", "Enlarger") && !session.list_has("Enlarger", "Grain"))
	testing.expect(t, session.list_has("", ""))
}
