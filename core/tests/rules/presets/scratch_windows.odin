#+build windows
package presets_tests

import win "core:sys/windows"

@(private)
remove_empty_folder :: proc(folder: string) {
	win.RemoveDirectoryW(win.utf8_to_wstring(folder, context.temp_allocator))
}
