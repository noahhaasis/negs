# Negs

[![Downloads](https://img.shields.io/github/downloads/noahhaasis/negs/total)](https://github.com/noahhaasis/negs/releases)

**Your photos, developed on film.**

Negs is a **free** photo app for your browser, Mac, Windows and Linux that runs your pictures through a **physical simulation of film**: the dyes, the print paper, the glow around highlights, the grain. They come out looking shot on film, not filtered to look like it.

Negs is built on two open-source projects: Andrea Volpato's [spektrafilm](https://github.com/andreavolpato/spektrafilm), the spectral film simulation behind every stock, and turbasvin's [spektrafilm-rs](https://github.com/turbasvin/spektrafilm-rs), the Rust port that runs it on the GPU. Negs improves on both: the GPU engine now matches the 64-bit reference render closely enough that exports run on the GPU by default, several times faster. It also adds Apple's RAW decoder and lab-style borders, and wraps it all in a web app and native apps for macOS, Windows and Linux.

> [!TIP]
> **Film simulation that keeps up with you.** Every slider updates live. Faster than spektrafilm-rs; dramatically faster than Python spektrafilm.

**[▶ Try it in your browser](https://getnegs.com/try/)** · nothing to install · your photos stay on your device

**[Download for Mac](https://github.com/noahhaasis/negs/releases/download/v0.9.1/Negs-0.9.1-macos-universal.dmg)** · macOS 14 or later · Apple silicon and Intel · [install steps](#macos)

**[Download for Windows](https://github.com/noahhaasis/negs/releases/download/v0.9.1/Negs-0.9.1-windows-x64-Setup.exe)** · Windows x64 · [portable version](https://github.com/noahhaasis/negs/releases/download/v0.9.1/Negs-0.9.1-windows-x64-portable.zip) · [install steps](#windows)

**Linux** · Arch and Omarchy, x86_64 · [one-command install](#linux-arch--omarchy)

[getnegs.com](https://getnegs.com)

| RAW · Apple default | Negs · Kodak Ektar 100 |
|---|---|
| ![Rome at dusk, the RAW as macOS renders it by default](docs/readme/compare-before.webp) | ![The same photo developed on Ektar 100 in Negs](docs/readme/compare-after.webp) |

## A darkroom in a window

![The Negs window: a photo of the Colosseum in a 35mm Kodak Ektar 100 frame, with film, camera and print controls on the right](docs/readme/app-window.webp)

In your browser, or as a native app on Mac, Windows and Linux. Pick a film, pick a paper, drag a slider, and watch the picture change as you move.

- Opens RAW files from most cameras, including Nikon HE★ (Mac only) and 60 MP medium format, plus JPEG, TIFF and PNG
- Crop, straighten, and add borders with each stock's real edge print
- Image Size enlarges a photo *before* the film, so grain and halation form at print size instead of being stretched afterwards
- Undo, keyboard shortcuts, pinch to zoom: it feels at home on Mac, Windows and Linux
- Exports PNG, TIFF or JPEG and keeps your camera's EXIF, at 8 or 16 bits per channel (JPEG with a quality slider), in sRGB, Display P3, Adobe RGB or ProPhoto RGB (16-bit only) with an embedded ICC profile
- Export the current film and paper colors as a 33³ sRGB `.cube` LUT from File → Export LUT… on every platform. No photograph is needed; spatial effects and Post edits are excluded.
- Live preview while you drag a slider, and what you see in the preview is what you export

https://github.com/user-attachments/assets/b1b50860-9160-4a6c-b939-42ceaa9e82cd

## Chemistry, not a filter

Negs doesn't guess what film looks like. It follows the light through the whole chain (film, enlarger, paper, scanner) using the published data for each stock.

- **Colour from the spectrum.** Your photo is turned back into light, exposed onto the film's three dye layers, printed onto paper and scanned. Colours shift the way they did on the real stock, because the same thing happens to them.
- **Halation.** Bright lights bleed a warm red glow, the way light bounces back through real film. Street lamps, sun on chrome, candle flames.
- **Grain that lives in the image.** Grain is simulated in each dye layer from how dense the film is at that spot. It sits inside the picture instead of being laid on top.
- **The developer's bite.** Development chemistry sharpens edges and deepens colour where tones meet, giving the crispness film has and digital lacks.
- **Paper matters.** The print paper changes the look as much as the film does, just like in a real lab.
- **Print it, or scan it.** Print the negative onto paper for the classic lab look, or skip the paper and scan the film directly.

### Why not a LUT?

A LUT looks at each pixel on its own and swaps its colour. Real film doesn't work like that: light spreads through the emulsion, highlights glow, grain follows exposure and neighbouring tones pull on each other. Negs simulates all of it, so the look holds up where LUTs fall apart: skin, neon, deep shadows, bright skies.

## Borders

Finish with a solid border or a thin keyline in any width and colour, a black film edge, or the full 35mm frame, sprocket holes and all, printed with the stock's own edge markings.

| White border | Keyline | Film edge | 35mm frame |
|---|---|---|---|
| ![A bench on a terrace above a lake and mountains, with a white border](docs/readme/border-white.webp) | ![Horse-drawn carriages in front of a palace, with a keyline border](docs/readme/border-keyline.webp) | ![A bar at night with glowing shelves, with a black film border](docs/readme/border-film.webp) | ![The Colosseum in a 35mm Kodak Gold 200 frame with sprocket holes and edge print](docs/readme/border-35mm.webp) |

## Your film shelf

| | |
|---|---|
| **Colour negative** | Portra 160, Portra 400, Portra 800 (+1, +2), Ektar 100, Gold 200, Ultramax 400, Fujicolor C200, Pro 400H, Superia X-TRA 400 |
| **Slide** | Ektachrome 100, Kodachrome 64, Velvia 100, Provia 100F |
| **Motion picture** | Vision3 50D, Vision3 250D, Vision3 200T, Vision3 500T, Verita 200D |
| **Black & white** | Tri-X 400 *(experimental)*, Double-X 5222 *(experimental)* |
| **Papers & print film** | Portra Endura, Endura Premier, Supra Endura, Ultra Endura, Ektacolor Edge, Crystal Archive II, Ilford Multigrade RC, Vision 2383, Vision 2393 |

Kodak, Fujifilm and Ilford stock names are trademarks of their owners. Negs is not affiliated with them.

## Install

### In your browser

No install needed: open **[getnegs.com/try](https://getnegs.com/try/)** in any recent browser. Negs runs entirely on your device, on the GPU through WebGPU where the browser supports it, so your photos are never uploaded.

[![Negs running in a browser: a photo of Rome at dusk on Kodak Portra 400, with film, paper and camera controls beside it](docs/readme/web-app.webp)](https://getnegs.com/try/)

### macOS

The first time you open Negs, macOS asks you to allow it in System Settings:

1. **Download** the `.dmg` from the [latest release](https://github.com/noahhaasis/negs/releases).
2. **Drag to Applications.** Open the `.dmg` and drag **Negs** into the **Applications** folder.
3. **Open it once.** Double-click Negs. macOS says it can't verify the app and won't open it. Click **Done** (not *Move to Trash*).
4. **Allow it in Settings.** Go to **System Settings → Privacy & Security**, scroll down and click **Open Anyway** next to the message about Negs. Enter your password, then click **Open**.

Why the extra step? Negs isn't notarized by Apple, which needs a paid developer account, so macOS asks you to allow it yourself the first time.

Negs updates itself: use **Negs → Check for Updates…**, or turn automatic checks on or off in Settings.

### Windows

Native Windows app with the same film engine as the Mac app: GPU previews, crop and straighten tools, borders, and Fast or Reference exports.

1. **Download** [`Negs-0.9.1-windows-x64-Setup.exe`](https://github.com/noahhaasis/negs/releases/download/v0.9.1/Negs-0.9.1-windows-x64-Setup.exe) and run it. It installs for your Windows account and adds Negs to **Open with**, without changing your default photo app.
2. **If Windows SmartScreen warns you**, click **More info → Run anyway**. Negs isn't code-signed, which needs a paid certificate.

Prefer no installer? Extract [`Negs-0.9.1-windows-x64-portable.zip`](https://github.com/noahhaasis/negs/releases/download/v0.9.1/Negs-0.9.1-windows-x64-portable.zip) and run `Negs.exe`. Keep the included helper executables and `data` folder beside it.

Windows doesn't update itself yet: download the newer installer from the [releases page](https://github.com/noahhaasis/negs/releases) and run it over the old one. The editing panel and film strip start enabled, and F7/F6 toggle them during use.

Windows packages can be built from source with [`windows/package.ps1`](windows/package.ps1); see the [Windows build instructions](docs/windows-app.md) and the [validation status](docs/windows-ui-validation.md).

### Linux (Arch / Omarchy)

Native GTK4 app on the same film engine, for x86_64 Arch Linux and Omarchy. Install it for your user (no root needed) with one command:

```sh
curl -fsSL https://raw.githubusercontent.com/noahhaasis/negs/main/linux/get-negs.sh | sh
```

It downloads the latest release, checks its SHA-256 checksum and installs under `~/.local`. It's built for rolling-release distributions: the release needs a very recent glibc (2.44), so it won't run on Ubuntu LTS, Debian stable or Fedora releases that ship older versions; there, [build from source](docs/linux-app.md). If you'd rather read it first, [the script](linux/get-negs.sh) is short. Negs needs GTK 4.16+, libadwaita 1.6+, OpenBLAS and a Vulkan driver for your GPU (the engine falls back to the CPU without one). On Omarchy these are normally present; elsewhere:

```sh
sudo pacman -S --needed gtk4 libadwaita openblas vulkan-icd-loader
```

Run `negs` (or `~/.local/bin/negs` if `~/.local/bin` isn't on your `PATH`) or open it from your launcher. To update, run the command again; to remove it, run `python3 ~/.local/share/negs/install.py --uninstall`. Your settings and photos are kept.

### Handy shortcuts

| | Mac | Windows & Linux |
|---|---|---|
| Open a photo | ⌘O | Ctrl+O |
| Export | ⇧⌘E | Ctrl+Shift+E |
| Save the preview | ⌘S | Ctrl+S |
| Rotate | ⌘R | Ctrl+R |
| Fit / actual size | ⌘0 / ⌘1 | Ctrl+0 / Ctrl+1 |
| Show or hide adjustments | ⌥⌘I | F7 |
| Show the original while held | \ | O or \ |

Pinch to zoom, scroll or drag to pan, double-click to fit.

## Questions

**Is Negs free?**
Yes. It's free to download and open source under the GPL-3.0 (see [License](#license)).

**macOS says Negs can't be opened. Is it safe?**
macOS shows that warning for every app that isn't notarized by Apple. Negs isn't, because notarizing needs a paid developer account. All of the source code is in this repository for anyone to check. Follow the [install steps](#macos): you only need to allow it once.

**Which files can I open?**
RAW files from most cameras, plus JPEG, TIFF and PNG. The Mac app uses Apple's RAW support with a rawler fallback; the Windows and Linux apps use rawler for RAW files.

**Does Negs respect my photo's colour profile?**
Yes. JPEG, PNG and TIFF files are read with their embedded ICC profile or colour tags (untagged files as sRGB), and RAW colours outside sRGB are kept. The preview develops in the colour space you export to, so on a wide-gamut display you see Display P3 or Adobe RGB colours while editing. For files with missing or wrong tags, Input Colour in the Lab tab lets you choose how they are read. A JPEG still carries its camera's tone curve and processing, which no profile undoes; RAW files give the film simulation the most to work with. Details: [colour management](docs/colour-management.md).

**Does it run on Intel Macs?**
Yes, on Intel Macs that run macOS 14 or later. Negs is a universal app, so it runs natively on both Intel and Apple silicon. It is developed and tested mainly on Apple silicon, so if something looks wrong on an Intel Mac, please [open an issue](https://github.com/noahhaasis/negs/issues).

**Is this a preset pack?**
No. Presets and LUTs remap colours. Negs simulates the film, the paper and the light, which is why halation, grain and edge effects come out right.

**Which platforms does it run on?**
Any recent browser, at [getnegs.com/try](https://getnegs.com/try/): nothing to install, and your photos never leave your device. As a desktop app: macOS 14 or later (Apple silicon and Intel), Windows x64, and x86_64 Arch and Omarchy Linux. There is a [Mac app](#macos), a [Windows app](#windows) with an installer and a portable build, and a native GTK4 app for [Linux](#linux-arch--omarchy).

**Found a bug or have an idea?**
[Open an issue](https://github.com/noahhaasis/negs/issues).

## Credits

The film model at the heart of Negs comes from two open-source projects. Negs builds fast apps on top of them: in the browser, and native on Mac, Windows and Linux.

- **[spektrafilm](https://github.com/andreavolpato/spektrafilm)** by Andrea Volpato: the spectral simulation of film and print. The physics and the stock data come from here.
- **[spektrafilm-rs](https://github.com/turbasvin/spektrafilm-rs)** by turbasvin: the Rust port of spektrafilm that Negs started from.

The default spectral-upsampling table is by [Johannes Hanatos](https://github.com/hanatos), author of [vkdt](https://github.com/hanatos/vkdt). The optional Arctic 2026 table is by Andrea Volpato, from spektrafilm, and the Mallett 2019 table is spektrafilm's tabulation of the spectral primary decomposition by Ian Mallett and Cem Yuksel (2019). The profiles and tables from spektrafilm are under CC BY-SA 4.0 ([license](data/SPEKTRAFILM_LICENSE.txt), [changes](data/CHANGELOG.txt)). Parts of the maths are ported from numpy, scipy, scikit-image and colour-science. RAW files Apple can't read are decoded with [rawler](https://github.com/dnglab/dnglab), and updates are delivered with [Sparkle](https://sparkle-project.org).

## License

Negs is free software under the **GNU General Public License, version 3** ([full text](LICENSE)), the same license as spektrafilm and spektrafilm-rs, which it builds on.

That means you can use Negs for anything, including commercial work. The photos you make with it are yours, and the license puts no conditions on them. You may also share and modify the app. If you distribute it, modified or not, you must pass on the same freedoms: release it under the GPL-3.0 and make the source code available.

- Copyright © Andrea Volpato (spektrafilm)
- Copyright © turbasvin (spektrafilm-rs)
- Copyright © 2026 Noah Haasis (Negs: the apps and the changes to the engine)

Third-party components keep their own licenses. Sparkle, the update framework bundled in the Mac app, is under the MIT license.

## For developers

Building from source, the command-line tools, accuracy against the original Python project, and how the code is laid out: see [docs/development.md](docs/development.md).

### Building the web app

The web app in [`web/`](web/) runs the same Rust film engine compiled to WebAssembly, with the WebGPU engine sharing the desktop apps' shaders. See [docs/web-wasm.md](docs/web-wasm.md) for how it's built and how closely it matches the native renders.

### Building the Linux app

The Linux app is a GTK4/libadwaita editor on the same Rust film engine. To build and run it, or to build an Arch package, see the [Linux build and installation details](docs/linux-app.md) and the [AUR/Omarchy release checklist](docs/linux-release.md).
