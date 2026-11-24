#+build darwin, linux
package presets_tests

import "core:strings"
import "core:sys/posix"

@(private)
remove_empty_folder :: proc(folder: string) {
	posix.rmdir(strings.clone_to_cstring(folder, context.temp_allocator))
}
