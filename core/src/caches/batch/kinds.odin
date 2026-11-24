// Batch export (windows/src/batch.rs, the Mac's BatchExport.swift): the
// choices of the batch dialog, the names a batch gives, the journal an
// unfinished batch leaves for the next launch, and the words of its status
// line and popover. The export itself (the engine) stays with the host.
//
// This file: the choices' kinds, and the save and export choices a batch
// keeps by key (windows/src/model.rs's Edge, ExportSize and SaveSettings,
// inspector_view/output.rs's OutputFormat, engine.rs's ExportEngine), in
// their `ALL` orders.
package batch

// `{name}_{film}`, `{name}`, or the user's own text with those tokens.
Naming :: enum u8 {
	Name_Film,
	Name,
	Custom,
}

NAMING_TITLES := [Naming]string {
	.Name_Film = "Name_Film",
	.Name      = "Name",
	.Custom    = "Custom",
}

// As the Mac stores them (`batchNaming`), and serde's camelCase names.
NAMING_KEYS := [Naming]string {
	.Name_Film = "nameFilm",
	.Name      = "name",
	.Custom    = "custom",
}

// What a batch does when a file of its name is already there.
Existing :: enum u8 {
	Number,
	Replace,
	Skip,
}

EXISTING_TITLES := [Existing]string {
	.Number  = "Add a Number",
	.Replace = "Replace",
	.Skip    = "Skip",
}

EXISTING_KEYS := [Existing]string {
	.Number  = "number",
	.Replace = "replace",
	.Skip    = "skip",
}

// How a photograph of a batch went.
Status :: enum u8 {
	Waiting,
	Done,
	Failed,
	Skipped,
}

STATUS_KEYS := [Status]string {
	.Waiting = "waiting",
	.Done    = "done",
	.Failed  = "failed",
	.Skipped = "skipped",
}

Output_Format :: enum u8 {
	Png,
	Jpeg,
	Tiff,
}

FORMAT_KEYS := [Output_Format]string {
	.Png  = "png",
	.Jpeg = "jpeg",
	.Tiff = "tiff",
}

FORMAT_EXTENSIONS := [Output_Format]string {
	.Png  = "png",
	.Jpeg = "jpg",
	.Tiff = "tif",
}

Edge :: enum u8 {
	Full,
	Long,
	Short,
}

EDGE_KEYS := [Edge]string {
	.Full  = "full",
	.Long  = "long",
	.Short = "short",
}

Export_Engine :: enum u8 {
	Reference,
	Fast,
}

ENGINE_KEYS := [Export_Engine]string {
	.Reference = "reference",
	.Fast      = "fast",
}

// The colour spaces a save offers; ProPhoto RGB at 8 bits is Adobe RGB.
COLOR_SPACES := [4]string{"sRGB", "Display P3", "Adobe RGB (1998)", "ProPhoto RGB"}
PROPHOTO :: 3
PROPHOTO_FALLBACK :: 2

// ExportSize::FULL's pixels: what a full-size choice keeps for later.
FULL_PIXELS :: 2048

// `Naming::from_key` and the others: the kind whose key it is, else the
// default (the first; OutputFormat's Png and Edge's Full are first too).
naming_from_key :: proc(key: string) -> Naming {
	for k, n in NAMING_KEYS {
		if k == key {
			return n
		}
	}
	return .Name_Film
}

existing_from_key :: proc(key: string) -> Existing {
	for k, e in EXISTING_KEYS {
		if k == key {
			return e
		}
	}
	return .Number
}

format_from_key :: proc(key: string) -> Output_Format {
	for k, f in FORMAT_KEYS {
		if k == key {
			return f
		}
	}
	return .Png
}

edge_from_key :: proc(key: string) -> Edge {
	for k, e in EDGE_KEYS {
		if k == key {
			return e
		}
	}
	return .Full
}

// ExportEngine::from_key: "reference", else Fast.
engine_from_key :: proc(key: string) -> Export_Engine {
	return .Reference if key == "reference" else .Fast
}

// `SaveSettings::effective_space`.
effective_space :: proc "contextless" (chosen: u64, bit_depth: u32) -> u64 {
	c := min(chosen, u64(len(COLOR_SPACES) - 1))
	return PROPHOTO_FALLBACK if c == PROPHOTO && bit_depth == 8 else c
}

// SaveSettings: bits per channel, colour space and JPEG quality.
Save :: struct {
	bit_depth:    u32,
	color_space:  u64,
	jpeg_quality: u32,
}
