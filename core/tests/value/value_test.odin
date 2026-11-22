// The serde_json-faithful value's own tests (core/src/value). What each
// rule means, with serde_json's own answers; core/diff/tests/value.rs
// holds everything to serde_json on random inputs. Run by `build.py test`.
package value_tests

import "core:strings"
import "core:testing"
import "../../src/value"
import "../../src/value/bridge"
import lib "../../src/library"

round_trip :: proc(t: ^testing.T, text: string, compact: string, loc := #caller_location) {
	v, err := value.parse_string(text)
	defer value.destroy(v)
	testing.expectf(t, err == .None, "%q: %v", text, err, loc = loc)
	out := value.to_string(v)
	defer delete(out)
	testing.expectf(t, out == compact, "%q wrote %q, not %q", text, out, compact, loc = loc)
}

refused :: proc(t: ^testing.T, text: string, loc := #caller_location) {
	v, err := value.parse_string(text)
	defer value.destroy(v)
	testing.expectf(t, err != .None, "%q was accepted", text, loc = loc)
}

@(test)
numbers_keep_their_kind :: proc(t: ^testing.T) {
	round_trip(t, "[0, -0, 1, -1, 1.0, 18446744073709551615, 18446744073709551616]", "[0,-0.0,1,-1,1.0,18446744073709551615,1.8446744073709552e19]")
	round_trip(t, "[-9223372036854775808, -9223372036854775809]", "[-9223372036854775808,-9.223372036854776e18]")
	round_trip(t, "[0.1, 1e16, 1.5e-7, 123456789.0, 1e15, 0.0001, 0.00001, 0.000001]", "[0.1,1e16,1.5e-7,123456789.0,1000000000000000.0,0.0001,0.00001,1e-6]")
	round_trip(t, "[1e-400, -1e-400, 0e999999999999, 2.2250738585072011e-308]", "[0.0,-0.0,0.0,2.225073858507201e-308]")
	v, _ := value.parse_string("[5, -5, 5.0]")
	defer value.destroy(v)
	a := v.(value.Array)
	_, is_u := a[0].(u64)
	_, is_i := a[1].(i64)
	_, is_f := a[2].(f64)
	testing.expect(t, is_u && is_i && is_f)
}

@(test)
long_mantissas_round_correctly :: proc(t: ^testing.T) {
	// 2^-1074 * 1.5 is halfway between the two smallest subnormals; just
	// above it rounds up, at it rounds to even (down).
	half := "2.4703282292062327208828439643411068618252990130716238221279284125033775363510437593264991818081799618989828234772285886546332835517796989819938739800539093906315035659515570226392290858392449105184435931802849936536152500319370457678249219365623669863658480757001585769269903706311928279558551332927834338409351978015531246597263579574622766465272827220056374006485499977096599470454020828166226237857393450736339007967761930577506740176324673600968951340535537458516661134223766678604162159680461914467291840300530057530849048765391711386591646239524912623653881879636239373280423891018672348497668235089863388587925628302755995657524455507255189313690836254779186948667994968324049705821028513185451396213837722826145437693412532098591327667236328125e-324"
	v, err := value.parse_string(half)
	testing.expect(t, err == .None)
	testing.expect_value(t, transmute(u64)v.(f64), 0)
	above, _ := strings.replace(half, "125e-324", "1250001e-324", 1)
	defer delete(above)
	w, err2 := value.parse_string(above)
	testing.expect(t, err2 == .None)
	testing.expect_value(t, transmute(u64)w.(f64), 1)
	refused(t, "1e309")
	refused(t, "-1.8e308")
	round_trip(t, "1.7976931348623157e308", "1.7976931348623157e308")
}

@(test)
texts_serde_refuses :: proc(t: ^testing.T) {
	for text in ([]string{"", " ", "01", "1.", ".5", "1e", "-", "+1", "[1,]", "{\"a\":1,}", "{1:2}", "nul", "1 2", "\"\\x\"", "\"a\x01\"", "\"\\ud800\"", "\"\\udc00\"", "\"\\ud800\\u0041\"", "[", "{\"a\"}", "NaN", "\"\xff\"", "\"\xed\xa0\x80\""}) {
		refused(t, text)
	}
	round_trip(t, " {\"b\":1,\"a\":[true,false,null],\"b\":2}\n", `{"a":[true,false,null],"b":2}`)
	round_trip(t, `"\ud83d\ude00\u00e9\/\b"`, "\"😀é/\\b\"")
	round_trip(t, "\"\x7f\\u0001\"", "\"\x7f\\u0001\"")
}

@(test)
nesting_stops_at_128 :: proc(t: ^testing.T) {
	deep :: proc(n: int) -> string {
		b := strings.builder_make()
		for _ in 0 ..< n {
			strings.write_byte(&b, '[')
		}
		for _ in 0 ..< n {
			strings.write_byte(&b, ']')
		}
		return strings.to_string(b)
	}
	ok := deep(127)
	defer delete(ok)
	bad := deep(128)
	defer delete(bad)
	v, err := value.parse_string(ok)
	value.destroy(v)
	testing.expect(t, err == .None)
	_, err = value.parse_string(bad)
	testing.expect(t, err == .Recursion_Limit)
}

@(test)
pretty_writes_as_serde :: proc(t: ^testing.T) {
	v, _ := value.parse_string(`{"a":[1,{}],"b":{},"c":[]}`)
	defer value.destroy(v)
	out := value.to_string_pretty(v)
	defer delete(out)
	testing.expect_value(t, out, "{\n  \"a\": [\n    1,\n    {}\n  ],\n  \"b\": {},\n  \"c\": []\n}")
}

@(test)
display_is_rusts :: proc(t: ^testing.T) {
	cases := [][2]string{{"1", "1"}, {"-0", "-0"}, {"0.1", "0.1"}, {"1e21", "1000000000000000000000"}, {"1e-7", "0.0000001"}, {"2.5", "2.5"}}
	for c in cases {
		v, _ := value.parse_string(c[0])
		n, _ := value.as_f64(v)
		if c[0] == "-0" {
			n = -0.0
		}
		text := value.display(n)
		testing.expectf(t, text == c[1], "%s: %s", c[0], text)
		delete(text)
	}
	inf := transmute(f64)u64(0x7FF0_0000_0000_0000)
	for c in ([][2]f64{{inf, 0}, {-inf, 0}}) {
		text := value.display(c[0])
		testing.expect(t, text == "inf" || text == "-inf")
		delete(text)
	}
}

@(test)
helpers_follow_json_rs :: proc(t: ^testing.T) {
	v, _ := value.parse_string(`{"scanner": {"unsharp_mask": [0.5, 1.0]}, "io": {"rotate_cw": 0}}`)
	defer value.destroy(v)
	testing.expect(t, value.equal(value.get(v, "scanner.unsharp_mask.1"), f64(1.0)))
	testing.expect(t, value.equal(value.get(v, "scanner.unsharp_mask.+1"), f64(1.0)))
	testing.expect(t, value.get(v, "missing.path") == nil)
	value.set(&v, "scanner.unsharp_mask.0", u64(2))
	value.set(&v, "io.new.deep", true)
	value.set(&v, "scanner.unsharp_mask.7", u64(3))
	out := value.to_string(v)
	defer delete(out)
	testing.expect_value(t, out, `{"io":{"new":{"deep":true},"rotate_cw":0},"scanner":{"unsharp_mask":[2,1.0]}}`)
	two := value.num(2.0)
	_, is_u := two.(u64)
	testing.expect(t, is_u)
	testing.expect(t, value.num(transmute(f64)u64(0x7FF8_0000_0000_0000)) == nil)
	a, _ := value.parse_string(`{"a":[1,2.5]}`)
	b, _ := value.parse_string(`{"a":[1.0,2.5]}`)
	defer value.destroy(a)
	defer value.destroy(b)
	testing.expect(t, value.same(a, b) && !value.equal(a, b))
}

@(test)
restoring_keeps_new_defaults_and_saved_nulls :: proc(t: ^testing.T) {
	d, _ := value.parse_string(`{"dir": {"langmuir": [1, 1, 1], "gamma": null}, "gamut": {"lightness": [0.95, 1, 1.6], "knee": 1}, "f": "x"}`)
	s, _ := value.parse_string(`{"dir": {"langmuir": [null, 0.5, null], "gamma": [0.52, 0.51, 0.26]}, "gamut": {"lightness": null, "knee": null}, "f": 7, "gone": 1}`)
	defer value.destroy(d)
	defer value.destroy(s)
	r := value.restoring(d, s, {"dir.langmuir[*]", "dir.gamma", "gamut.lightness"})
	defer value.destroy(r)
	out := value.to_string(r)
	defer delete(out)
	testing.expect_value(t, out, `{"dir":{"gamma":[0.52,0.51,0.26],"langmuir":[null,0.5,null]},"f":"x","gamut":{"knee":1,"lightness":null}}`)
}

@(test)
library_json_crosses_both_ways :: proc(t: ^testing.T) {
	v, _ := value.parse_string(`{"a":[1,2.5,-3,true,null,"s"],"b":{}}`)
	defer value.destroy(v)
	j := bridge.to_library(v)
	defer lib.json_destroy(j)
	back := bridge.from_library(j)
	defer value.destroy(back)
	testing.expect(t, value.equal(v, back))
}
