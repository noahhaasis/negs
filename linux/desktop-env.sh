#!/usr/bin/env bash
# Optional validation helper for a resumed/SSH shell on the logged-in desktop.
if [[ -z "${XDG_RUNTIME_DIR:-}" ]]; then
  export XDG_RUNTIME_DIR="/run/user/$(id -u)"
fi
if [[ -z "${WAYLAND_DISPLAY:-}" ]]; then
  read -r negs_socket negs_instance < <(hyprctl -j instances | python3 -c 'import json,sys; rows=json.load(sys.stdin); assert len(rows)==1, "Select the target desktop explicitly"; print(rows[0]["wl_socket"], rows[0]["instance"])')
  if [[ -z "${negs_socket:-}" || ! -S "$XDG_RUNTIME_DIR/$negs_socket" ]]; then
    printf 'No unambiguous live Wayland session was found.\n' >&2
    return 1
  fi
  export WAYLAND_DISPLAY="$negs_socket" HYPRLAND_INSTANCE_SIGNATURE="$negs_instance"
  unset negs_socket negs_instance
fi
export XDG_SESSION_TYPE=wayland
if [[ -z "${DBUS_SESSION_BUS_ADDRESS:-}" && -S "$XDG_RUNTIME_DIR/bus" ]]; then
  export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"
fi
