// What Original does in the photo view (`OriginalMode`), and what it shows
// of a RAW shot as RAW+JPEG, both remembered under the Mac's keys.
package compare

import "core:fmt"

Original_Mode :: enum u32 {
	// Hold it to see the whole original.
	Hold,
	// A click shows the original left of a line that can be dragged.
	Split,
	// A click compares the photograph with its original.
	Side_By_Side,
}

original_mode_key :: proc "contextless" (mode: Original_Mode) -> cstring {
	switch mode {
	case .Hold:
		return "hold"
	case .Split:
		return "split"
	case .Side_By_Side:
		return "sideBySide"
	}
	return "hold"
}

// The mode a key names; Hold for anything else.
original_mode_from_key :: proc "contextless" (key: string) -> Original_Mode {
	for mode in Original_Mode {
		if string(original_mode_key(mode)) == key {
			return mode
		}
	}
	return .Hold
}

original_mode_title :: proc "contextless" (mode: Original_Mode) -> cstring {
	switch mode {
	case .Hold:
		return "Hold to see the whole"
	case .Split:
		return "Split line"
	case .Side_By_Side:
		return "Side by side"
	}
	return ""
}

// The Original word's tooltip; `original` is "original" or "camera JPEG".
original_mode_help :: proc(mode: Original_Mode, original: string, allocator := context.allocator) -> string {
	switch mode {
	case .Hold:
		return fmt.aprintf("Hold to show the %s (\\ or O). Hold T to turn the photograph upside down.", original, allocator = allocator)
	case .Split:
		return fmt.aprintf("Show the %s left of a line you can drag. Hold \\ to see the whole %s.", original, original, allocator = allocator)
	case .Side_By_Side:
		return fmt.aprintf("Compare with the %s, side by side. Hold \\ to see the whole %s.", original, original, allocator = allocator)
	}
	return ""
}

Original_Reference :: enum u32 {
	Raw,
	Camera_Jpeg,
}

original_reference_key :: proc "contextless" (reference: Original_Reference) -> cstring {
	return "cameraJPEG" if reference == .Camera_Jpeg else "raw"
}

// The reference a key names; Raw for anything else.
original_reference_from_key :: proc "contextless" (key: string) -> Original_Reference {
	for reference in Original_Reference {
		if string(original_reference_key(reference)) == key {
			return reference
		}
	}
	return .Raw
}

original_reference_title :: proc "contextless" (reference: Original_Reference) -> cstring {
	return "Show the camera JPEG" if reference == .Camera_Jpeg else "Show the RAW"
}
