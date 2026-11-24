// Solved Match camera brightness values, kept across photographs and
// launches (windows/src/match_cache.rs, the Mac's MatchCache.swift; ticket
// 39). Each value is named by everything its solve read: the file as it is
// on disk (path, size, modification time), the exact request the helper
// solved (film, paper and every parameter) and the engine (app version,
// helper binary, data folder). `BrightnessMatches.json` in the cache
// folder; past `limit` the least recently used go.
//
// The cache here is one value used from one thread at a time: the host
// keeps it behind its lock and schedules the delayed write (`store` says
// when one is due); `flush_bytes` is the part of `flush` under the lock,
// `write_file` the part after it. Times are the host's (`now`, seconds
// since 1970 as `as_secs_f64`), so a test can give its own.
package match_cache

import "core:slice"
import "core:strings"
import "../../value"
import files "../files"
import "../serde"

LIMIT :: 2000

Solved :: struct {
	ev:      f64,
	limited: bool,
	// When it was last solved or used, for the LRU.
	used:    f64,
}

Cache :: struct {
	file:            string,
	limit:           u64,
	// Read on first use (`loaded`).
	loaded:          bool,
	entries:         map[string]Solved,
	dirty:           bool,
	write_scheduled: bool,
	allocator:       value.Allocator,
}

// `MatchCache::new`: nothing read yet.
cache_make :: proc(file: string, limit: u64, allocator := context.allocator) -> Cache {
	return Cache{file = strings.clone(file, allocator), limit = limit, entries = make(map[string]Solved, 16, allocator), allocator = allocator}
}

@(private = "file")
clear_entries :: proc(c: ^Cache) {
	for k in c.entries {
		delete(k, c.allocator)
	}
	clear(&c.entries)
}

cache_destroy :: proc(c: ^Cache) {
	clear_entries(c)
	delete(c.entries)
	delete(c.file, c.allocator)
}

// `load`: the file's entries the first time, none if it doesn't read as a
// map of solves (serde_json's `from_slice`, `unwrap_or_default`).
load :: proc(c: ^Cache) {
	if c.loaded {
		return
	}
	c.loaded = true
	bytes, err := files.read(c.file, context.temp_allocator)
	if err != 0 {
		return
	}
	if !parse_entries(bytes, c) {
		clear_entries(c)
	}
}

// The solve kept for `key`, marked as used at `now`.
solved :: proc(c: ^Cache, key: string, now: f64) -> (ev: f64, limited: bool, found: bool) {
	load(c)
	entry, ok := &c.entries[key]
	if !ok {
		return 0, false, false
	}
	entry.used = now
	return entry.ev, entry.limited, true
}

// `store`: keeps a solve; past the limit the least recently used go, a
// tenth more than needed so it isn't sorted on every store. True when the
// host should schedule a write (none was).
store :: proc(c: ^Cache, key: string, ev: f64, limited: bool, now: f64) -> (schedule: bool) {
	load(c)
	if entry, ok := &c.entries[key]; ok {
		entry^ = Solved{ev, limited, now}
	} else {
		c.entries[strings.clone(key, c.allocator)] = Solved{ev, limited, now}
	}
	n := u64(len(c.entries))
	if n > c.limit {
		Aged :: struct {
			key:  string,
			used: f64,
		}
		oldest := make([]Aged, n, context.temp_allocator)
		i := 0
		for k, v in c.entries {
			oldest[i] = Aged{k, v.used}
			i += 1
		}
		// total_cmp, and the key for a tie (Rust's order there is the
		// HashMap's, which is random).
		slice.sort_by(oldest, proc(a, b: Aged) -> bool {
			if total_less(a.used, b.used) {
				return true
			}
			if total_less(b.used, a.used) {
				return false
			}
			return a.key < b.key
		})
		excess := n - c.limit + c.limit / 10
		for a in oldest[:min(excess, n)] {
			k, _ := delete_key(&c.entries, a.key)
			delete(k, c.allocator)
		}
	}
	c.dirty = true
	if !c.write_scheduled {
		c.write_scheduled = true
		return true
	}
	return false
}

// f64::total_cmp's less-than.
@(private = "file")
total_less :: proc "contextless" (a, b: f64) -> bool {
	x := transmute(i64)a
	y := transmute(i64)b
	x ~= i64(u64(x >> 63) >> 1)
	y ~= i64(u64(y >> 63) >> 1)
	return x < y
}

// The locked half of `flush`: no write is scheduled any more; if anything
// isn't on disk, the entries as serde_json writes the map (keys sorted
// here; a HashMap's order is random), in `allocator`.
flush_bytes :: proc(c: ^Cache, allocator := context.allocator) -> (bytes: []u8, write: bool) {
	c.write_scheduled = false
	if !c.dirty {
		return nil, false
	}
	c.dirty = false
	if !c.loaded {
		// Rust's `entries` is none until read; nothing to write then.
		return nil, false
	}
	return transmute([]u8)entries_to_string(c, allocator), true
}

// The unlocked half: the folder made (errors ignored), `{stem}.tmp`
// written and moved over the file, the staging file removed.
write_file :: proc(file: string, bytes: []u8) {
	if folder, ok := files.parent(file); ok {
		files.create_dir_all(folder)
	}
	staging := files.with_extension(file, "tmp", context.temp_allocator)
	if files.write(staging, bytes) == 0 {
		files.replace(staging, file)
	}
	files.remove(staging)
}

// `#[cfg(test)] count`: the entries, read first if need be.
count :: proc(c: ^Cache) -> int {
	load(c)
	return len(c.entries)
}

// MARK: The file

SOLVED_FIELDS := [?]string{"ev", "limited", "used"}

@(private = "file")
read_solved :: proc(p: ^value.Parser) -> (Solved, bool) {
	out: Solved
	field :: proc(p: ^value.Parser, index: int, data: rawptr) -> bool {
		s := (^Solved)(data)
		switch index {
		case 0:
			s.ev = serde.read_f64(p) or_return
		case 1:
			s.limited = serde.read_bool(p) or_return
		case 2:
			s.used = serde.read_f64(p) or_return
		}
		return true
	}
	f, ok := serde.read_struct(p, SOLVED_FIELDS[:], &out, field)
	if ok {
		ok = serde.has_fields(p, f, {0, 1, 2}, {})
	}
	return out, ok
}

// `HashMap<String, Solved>` from serde_json: an object, the last of a
// repeated key winning, into `c.entries`.
parse_entries :: proc(text: []u8, c: ^Cache) -> bool {
	p := value.parser_make(text, context.temp_allocator)
	ch, any := value.peek_ws(&p)
	if !any || ch != '{' {
		return false
	}
	value.enter(&p) or_return
	p.i += 1
	first := true
	for {
		more := value.map_next(&p, &first) or_return
		if !more {
			break
		}
		key := value.map_key(&p) or_return
		s := read_solved(&p) or_return
		if entry, found := &c.entries[key]; found {
			entry^ = s
		} else {
			c.entries[strings.clone(key, c.allocator)] = s
		}
	}
	value.leave(&p)
	value.map_end(&p) or_return
	return value.finish(&p)
}

// `serde_json::to_vec(&entries)`, keys sorted.
entries_to_string :: proc(c: ^Cache, allocator := context.allocator) -> string {
	keys := make([]string, len(c.entries), context.temp_allocator)
	i := 0
	for k in c.entries {
		keys[i] = k
		i += 1
	}
	slice.sort(keys)
	b := strings.builder_make(0, 64 + 120 * len(keys), allocator)
	strings.write_byte(&b, '{')
	for k, n in keys {
		if n > 0 {
			strings.write_byte(&b, ',')
		}
		s := c.entries[k]
		value.write_string(&b, k)
		strings.write_string(&b, `:{"ev":`)
		serde.write_f64(&b, s.ev)
		strings.write_string(&b, `,"limited":`)
		serde.write_bool(&b, s.limited)
		strings.write_string(&b, `,"used":`)
		serde.write_f64(&b, s.used)
		strings.write_byte(&b, '}')
	}
	strings.write_byte(&b, '}')
	return strings.to_string(b)
}
