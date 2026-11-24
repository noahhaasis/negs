/* Phase 4b's structs: presets, session. */
#include "layout_check.h"

void check_4b(void) {
    SIZE(negs_preset_file_t);
    FIELD(negs_preset_file_t, path);
    FIELD(negs_preset_file_t, path_len);
    FIELD(negs_preset_file_t, size);
    FIELD(negs_preset_file_t, modified_s);
    FIELD(negs_preset_file_t, modified_ns);
    SIZE(negs_preset_list_t);
    FIELD(negs_preset_list_t, files);
    FIELD(negs_preset_list_t, count);
}
