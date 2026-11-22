// A visit to the tray (`TrayVisit`): trying, keeping and going back, what
// that does to Undo; and the keys the open tray takes, and Esc's order.
package tray

// The undo name for keeping a stock.
visit_action :: proc "contextless" (paper, read_as: bool) -> cstring {
	if paper {
		return "Change Paper"
	} else if read_as {
		return "Change Negative Film"
	}
	return "Change Film"
}

// Something else than the stock it opened on is being tried.
visit_tried :: proc "contextless" (before_id, current: string) -> bool {
	return current != before_id
}

// The tile of `stock` says "before": another one is being tried.
visit_shows_before :: proc "contextless" (before_id, stock, current: string) -> bool {
	return visit_tried(before_id, current) && stock == before_id
}

// What ending a visit does to the photograph and to Undo.
Ending :: enum u32 {
	// Kept what it opened on, or went back to it: nothing.
	Nothing,
	// Kept another stock: one undo step back to the edit before the visit.
	Step,
	// Went back: the edit before the visit returns, without an undo step.
	Restore,
}

// Return, Done, a click outside, a command or any other edit keep the stock
// being tried (`keep`); Esc goes back. `unchanged`: the visit's edit before
// equals the edit now (with the app's preferences in it).
ending :: proc "contextless" (keep, unchanged: bool) -> Ending {
	if unchanged {
		return .Nothing
	}
	return .Step if keep else .Restore
}

// Return in Search: tries the first match; with nothing typed it keeps.
Submit :: enum u32 {
	Keep,
	Try_First,
}

submit :: proc "contextless" (query: string) -> Submit {
	return .Try_First if searching(query) else .Keep
}

// Esc in Search: clears it; in an empty Search it goes back, as anywhere in
// the tray.
Cancel :: enum u32 {
	Clear,
	Go_Back,
}

cancel :: proc "contextless" (query: string) -> Cancel {
	return .Go_Back if len(query) == 0 else .Clear
}

// A key the tray may take.
Key :: enum u32 {
	Escape,
	Return,
	Left,
	Right,
	Other,
}

// What a key does while the tray is open.
Tray_Key :: enum u32 {
	// Esc: back to the stock from before the tray opened, and close.
	Go_Back,
	// Return: keep the stock being tried, and close.
	Keep,
	// ← →: the previous or next stock (the step's sign).
	Step,
	// Not the tray's: typing keeps its keys, and Ctrl or Alt are commands.
	Pass,
}

// While the tray is open, Esc goes back, Return keeps, and ← → step through
// the stocks rather than the photographs. Typing keeps its keys.
tray_key :: proc "contextless" (key: Key, command, typing: bool) -> (action: Tray_Key, step: i32) {
	if command || typing {
		return .Pass, 0
	}
	switch key {
	case .Escape:
		return .Go_Back, 0
	case .Return:
		return .Keep, 0
	case .Left:
		return .Step, -1
	case .Right:
		return .Step, 1
	case .Other:
		return .Pass, 0
	}
	return .Pass, 0
}

// What's open when Esc is pressed (§5.2) (negs_tray_escape_context_t).
Escape_Context :: struct {
	alert, typing, tray, cropping, grid, image_only, photograph: bool,
}

// What Esc does, the first that applies.
Escape :: enum u32 {
	// The alert closes.
	Alert,
	// The field puts the old value back.
	Typing,
	// The tray goes back to the earlier stock and closes.
	Tray,
	// Cropping cancels.
	Crop,
	// The grid folds back.
	Grid,
	// Image Only ends.
	Image_Only,
	// The photograph view opens the grid.
	Show_Grid,
	Nothing,
}

escape :: proc "contextless" (c: Escape_Context) -> Escape {
	switch {
	case c.alert:
		return .Alert
	case c.typing:
		return .Typing
	case c.tray:
		return .Tray
	case c.cropping:
		return .Crop
	case c.grid:
		return .Grid
	case c.image_only:
		return .Image_Only
	case c.photograph:
		return .Show_Grid
	}
	return .Nothing
}
