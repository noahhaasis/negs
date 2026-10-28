#!/usr/bin/env bash
# Run an AppImage on clean distribution containers with an ordinary desktop
# library set but no GTK 4, libadwaita or OpenBLAS: payload and CPU engine
# checks, then a GUI start and quit under Xvfb. Containers have no FUSE, so
# this exercises the runtime's extract-and-run path; normal FUSE mounting
# needs a real desktop test.
#   linux/appimage/smoke.sh APPIMAGE [ubuntu fedora arch]
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
image=$(readlink -f "$1")
shift
systems=("${@:-ubuntu fedora arch}")
read -r -a systems <<< "${systems[*]}"
run() { # container-image install-command
    docker run --rm -v "$image:/negs.AppImage:ro" -v "$root/linux:/checks/linux:ro" \
        -v "$root/scripts:/checks/scripts:ro" "$1" bash -euo pipefail -c "
        $2 >/dev/null
        for lib in libgtk-4.so.1 libadwaita-1.so.0 libopenblas.so.0; do
            if ldconfig -p | grep -q \"\$lib\"; then echo \"host unexpectedly has \$lib\"; exit 1; fi
        done
        cd /tmp && /negs.AppImage --appimage-extract >/dev/null
        OPENBLAS_NUM_THREADS=1 python3 /checks/linux/verify_package.py --prefix /tmp/squashfs-root/usr --format appimage
        set +e
        NEGS_PROFILE_DIR=/tmp/profile NEGS_QUIT_AFTER_MS=12000 SPEKTRAFILM_BACKEND=cpu \
            timeout 120 xvfb-run -a /negs.AppImage --appimage-extract-and-run --fresh > /tmp/gui.log 2>&1
        status=\$?
        set -e
        cat /tmp/gui.log
        if [ \$status -ne 0 ] || grep -E 'CRITICAL|panicked|cannot open shared object|symbol lookup error' /tmp/gui.log; then
            echo \"GUI smoke test failed (exit \$status)\"; exit 1
        fi"
}
desktop_debian='libglib2.0-0t64 libpango-1.0-0 libpangocairo-1.0-0 libpangoft2-1.0-0 libcairo2 libcairo-gobject2
    libgdk-pixbuf-2.0-0 libharfbuzz0b libfontconfig1 libwayland-client0 libwayland-egl1 libwayland-cursor0
    libxkbcommon0 libx11-6 libxext6 libxi6 libxrandr2 libxcursor1 libxdamage1 libxfixes3 libxinerama1
    libxrender1 libegl1 libgl1 libgles2 libtiff6 libvulkan1 fonts-dejavu-core xvfb xauth python3 binutils'
desktop_debian=$(echo $desktop_debian)
for system in "${systems[@]}"; do
    echo "== $system"
    case $system in
        ubuntu) run ubuntu:24.04 "apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends $desktop_debian" ;;
        fedora) run fedora:43 "dnf install -y --setopt=install_weak_deps=False glib2 pango cairo cairo-gobject gdk-pixbuf2 harfbuzz fontconfig libwayland-client libwayland-egl libwayland-cursor libxkbcommon libX11 libXext libXi libXrandr libXcursor libXdamage libXfixes libXinerama libXrender mesa-libEGL mesa-libGL libglvnd-gles libtiff vulkan-loader dejavu-sans-fonts xorg-x11-server-Xvfb xorg-x11-xauth xvfb-run python3 binutils findutils" ;;
        arch) run archlinux:base "pacman -Syu --noconfirm --needed glib2 pango cairo gdk-pixbuf2 harfbuzz fontconfig wayland libxkbcommon libx11 libxext libxi libxrandr libxcursor libxdamage libxfixes libxinerama libxrender libglvnd mesa libtiff vulkan-icd-loader ttf-dejavu xorg-server-xvfb xorg-xauth python binutils" ;;
        *) echo "Unknown system $system"; exit 1 ;;
    esac
done
echo 'AppImage smoke tests passed.'
