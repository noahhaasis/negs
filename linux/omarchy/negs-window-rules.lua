-- Negs photographs must remain opaque in every window state.
-- GTK cannot counteract compositor-wide opacity applied to its Wayland surface.
-- Apply after Omarchy's default rules, including the default-opacity tag rule.
hl.window_rule({
  -- Matches the current ID and the ID Negs used before 0.11.
  match = { class = "^(io[.]github[.]noahhaasis[.]negs|dev[.]negs[.]app)([.]UiPreview)?$" },
  tag = "-default-opacity",
  opacity = "1 override 1 override 1 override",
})
