// The film strip's and the grid's rules (windows/src/sheet_rules.rs;
// `ScrollWatch.swift`, `Selection.swift`, `SheetWords`, `SheetFlight.swift`):
// where every frame and thumbnail goes, the order thumbnails are read and
// developed in, how many at once, the grid's sentence and foot, the fade and
// the scroll bar. Lengths are DIPs, the Mac's points.
//
// Every constant here is exact in binary or only ever meets runtime values,
// so Odin's exact folding of constant expressions can't move a bit.
package sheet

// MARK: Sizes (Theme.Size)

// The strip: its top padding, the picture and the 12 DIP rebate under it.
STRIP :: 158.0
STRIP_TOP :: 6.0
STRIP_REBATE :: 12.0
STRIP_GAP :: 5.0
// The picture's height: the strip less its top padding and rebate.
PICTURE :: STRIP - STRIP_TOP - STRIP_REBATE
// The status line under the strip.
STATUS :: 28.0
// The strip and the status line: as tall as the tray.
BAND :: STRIP + STATUS
// Frames other than the current one; under the pointer; pressed.
STRIP_DIM :: 0.42
STRIP_HOVER :: 0.75
STRIP_PRESSED :: 0.9
// The pencil under the current frame, 3 DIP into the rebate.
PENCIL_WIDTH :: 4.0
PENCIL_DROP :: 3.0
// A photograph's width ÷ height before its picture is read.
FALLBACK_RATIO :: 1.5
// Frames are drawn this far either side of the strip's view.
STRIP_MARGIN :: 400.0

// The strip's handle (`StripLeader`).
LEADER_WIDTH :: 92.0
LEADER_HEIGHT :: 24.0
LEADER_RAISED :: 30.0
LEADER_SLANT :: 28.0

// The smoothstep glide back to the middle after a resize.
GLIDE_MS :: 350.0

// MARK: The grid

GRID_GAP :: 6.0
// The grid's padding: room for a box on the first and last rows.
GRID_TOP :: 24.0
GRID_SIDE :: 16.0
GRID_BOTTOM :: 24.0
// The rows' chosen height (the foot's Size), and Ctrl+plus/minus's step.
GRID_SIZE_MIN :: 100.0
GRID_SIZE_MAX :: 360.0
GRID_DEFAULT :: 176.0
GRID_SIZE_STEP :: 40.0
// Cells are drawn this far above and below the grid's view.
GRID_MARGIN :: 300.0
// The sentence over the grid, and the foot under it.
SENTENCE :: 46.0
FOOT :: STATUS
// The room kept round a box scrolled into view.
BOX_MARGIN :: 10.0
// How far the grid's yellow box reaches past its picture.
SELECTION_OUTSET :: 8.0

// MARK: Thumbnails

// How far either side of the middle of the view thumbnails are read ahead.
PREFETCH_REACH :: 32
// Photographs' shapes are read from their headers this many at a time.
HEADER_BATCH :: 48
// The developed thumbnail's long side, border included, and the cache's size.
EDITED_LONG_SIDE :: 512
EDITED_LIMIT :: 1 << 30
// Each photograph's last develop, screen-sized, kept as it's left.
RENDER_LONG_SIDE :: 2560
RENDERS_LIMIT :: 2 << 30

// MARK: The fade and scrolling

// The grid fades in over the photograph as it rises a little, and out again.
FADE_MS :: 150.0
FADE_RISE :: 14.0
// A wheel's notch glides the grid there, quickly, then easing off.
WHEEL_STEP :: 100.0
WHEEL_TAU_MS :: 55.0
// The overlay scroll bar at the grid's right edge.
BAR_WIDTH :: 7.0
BAR_WIDE :: 11.0
BAR_INSET :: 2.0
BAR_MIN :: 24.0
BAR_REACH :: 16.0
BAR_SHOWN_MS :: 1000.0
BAR_FADE_MS :: 300.0

// A reject's picture: greyed, more in the strip (0.22, 0.5 for the current
// frame) than in the grid (0.4) (`PhotoMarks.rejectOpacity`).
reject_opacity :: proc "contextless" (in_strip, current: bool) -> f64 {
	if in_strip {
		return 0.5 if current else 0.22
	}
	return 0.4
}

// How the strip shows a frame: the current and selected ones whole, the
// rest dimmed, lifted under the pointer, a reject greyed further.
strip_opacity :: proc "contextless" (current, selected, rejected, hovered, pressed: bool) -> f64 {
	if rejected {
		return reject_opacity(true, current || selected)
	}
	if current || selected {
		return 1.0
	} else if pressed {
		return STRIP_PRESSED
	} else if hovered {
		return STRIP_HOVER
	}
	return STRIP_DIM
}
