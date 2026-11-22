/* Phase 3b's structs: Picture rules: crop, border, exact fit, thumbnail compose, histogram, negative (core/src/rules). */
#include "layout_check.h"

void check_3b(void) {
    SIZE(negs_size_t);
    FIELD(negs_size_t, width);
    FIELD(negs_size_t, height);
    SIZE(negs_crop_settings_t);
    FIELD(negs_crop_settings_t, rect);
    FIELD(negs_crop_settings_t, custom);
    FIELD(negs_crop_settings_t, angle);
    FIELD(negs_crop_settings_t, aspect);
    FIELD(negs_crop_settings_t, reserved);
    SIZE(negs_orientation_t);
    FIELD(negs_orientation_t, turns);
    FIELD(negs_orientation_t, flip);
    FIELD(negs_orientation_t, reserved);
    SIZE(negs_crop_handle_t);
    FIELD(negs_crop_handle_t, x);
    FIELD(negs_crop_handle_t, y);
    SIZE(negs_crop_session_t);
    FIELD(negs_crop_session_t, rect);
    FIELD(negs_crop_session_t, custom);
    FIELD(negs_crop_session_t, photo);
    FIELD(negs_crop_session_t, intended);
    FIELD(negs_crop_session_t, angle);
    FIELD(negs_crop_session_t, rotation);
    FIELD(negs_crop_session_t, aspect);
    FIELD(negs_crop_session_t, portrait);
    FIELD(negs_crop_session_t, flip);
    FIELD(negs_crop_session_t, straightening);
    FIELD(negs_crop_session_t, has_intended);
    FIELD(negs_crop_session_t, reserved);
    SIZE(negs_border_settings_t);
    FIELD(negs_border_settings_t, width);
    FIELD(negs_border_settings_t, film_width);
    FIELD(negs_border_settings_t, line);
    FIELD(negs_border_settings_t, style);
    FIELD(negs_border_settings_t, color);
    FIELD(negs_border_settings_t, line_color);
    FIELD(negs_border_settings_t, enabled);
    FIELD(negs_border_settings_t, export_border);
    FIELD(negs_border_settings_t, instagram);
    FIELD(negs_border_settings_t, reserved);
    SIZE(negs_histogram_t);
    FIELD(negs_histogram_t, red);
    FIELD(negs_histogram_t, green);
    FIELD(negs_histogram_t, blue);
    SIZE(negs_scan_light_t);
    FIELD(negs_scan_light_t, value);
    FIELD(negs_scan_light_t, has);
    FIELD(negs_scan_light_t, reserved);
}
