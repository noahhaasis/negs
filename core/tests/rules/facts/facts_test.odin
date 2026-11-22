// core/src/rules/facts: windows/src/photo_facts.rs's own tests.
package facts_tests

import "core:testing"
import "../../../src/rules/facts"

@(private)
camera :: proc(t: ^testing.T, make, model: Maybe(string), want: Maybe(string)) {
	mk, has_make := make.?
	md, has_model := model.?
	got, outcome := facts.camera(mk, has_make, md, has_model)
	defer if outcome == .Some {
		delete(got)
	}
	w, has_want := want.?
	testing.expect_value(t, outcome == .Some, has_want)
	if has_want {
		testing.expect_value(t, got, w)
	}
}

@(test)
cameras_read_as_on_the_mac :: proc(t: ^testing.T) {
	camera(t, "NIKON CORPORATION", "NIKON Z f", "Nikon Z f")
	camera(t, "SONY", "ILCE-7M3", "Sony ILCE-7M3")
	camera(t, "NIKON CORPORATION", "NIKON D750", "Nikon D750")
	camera(t, "Canon", "Canon EOS R5", "Canon EOS R5")
	camera(t, nil, "X100V", "X100V")
	camera(t, "FUJIFILM", nil, "Fujifilm")
	camera(t, nil, nil, nil)
	// Rust's casing: final sigma, and a model cut off a char boundary.
	camera(t, "ΟΔΥΣΣΕΥΣ", nil, "Οδυσσευς")
	_, outcome := facts.camera("K", true, "kx", true)
	testing.expect_value(t, outcome, facts.Outcome.Panics)
}

@(private)
items :: proc(t: ^testing.T, f: facts.Facts, decimal: rune, want: []string) {
	p := facts.parts(f, decimal)
	defer facts.parts_destroy(&p)
	got: [dynamic]string
	defer delete(got)
	for i in 0 ..< 5 {
		if p.present[i] {
			append(&got, p.text[i])
		}
	}
	testing.expect_value(t, len(got), len(want))
	for w, i in want {
		if i < len(got) {
			testing.expect_value(t, got[i], w)
		}
	}
}

@(test)
items_are_formatted_with_the_locales_decimal :: proc(t: ^testing.T) {
	f := facts.Facts {
		camera           = "Nikon D750",
		has_camera       = true,
		iso              = 400,
		has_iso          = true,
		exposure         = 1.0 / 160.0,
		has_exposure     = true,
		aperture         = 4,
		has_aperture     = true,
		focal_length     = 100,
		has_focal_length = true,
	}
	items(t, f, ',', {"Nikon D750", "ISO 400", "1/160", "f/4", "100 mm"})
	g := facts.Facts {
		aperture         = 2.8,
		has_aperture     = true,
		exposure         = 1.3,
		has_exposure     = true,
		focal_length     = 23,
		has_focal_length = true,
	}
	items(t, g, ',', {"1,3 s", "f/2,8", "23 mm"})
	for c in ([?]struct {
			s:    f64,
			want: string,
		}{{2, "2 s"}, {0.5, "1/2"}, {0.4, "0.4 s"}}) {
		s := facts.shutter(c.s, '.')
		testing.expect_value(t, s, c.want)
		delete(s)
	}
}
