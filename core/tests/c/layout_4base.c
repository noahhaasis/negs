/* Phase 4 base's structs: value, edit state (core/src/value, core/src/state). */
#include "layout_check.h"

void check_4base(void) {
    SIZE(negs_str_t);
    FIELD(negs_str_t, data);
    FIELD(negs_str_t, len);
}
