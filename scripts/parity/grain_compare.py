"""Grain, Negs against Python experimental, by its statistics.

The random streams differ per engine (NumPy in Python, StdRng on the CPU,
per-pixel PCG on the GPU), so grain can't match pixel for pixel. What has to
match is how strong it is and how large it is. Each case renders flat grey
tiles at four exposures (auto exposure, glare off; everything else at the
stock's defaults, grain preset included) and measures, in the middle of each
tile, the RMS of the sRGB output in 8-bit steps and the grain's
autocorrelation width, from the radially averaged power spectrum.

Cases cover equal sizes (Python's 640 px GUI preview and a 2016 px frame)
and early enlargement (Python `io.upscale_factor` 2, Negs Image Size at twice
the longest side). `--view` adds what a person sees: Negs' full-size render
shown at a window's size, beside Python's 640 px preview enlarged to it.
"""

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parent))
from check_parity import apply_params, python_overrides  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
LEVELS = [0.02, 0.06, 0.18, 0.54]
STOCKS = [
    ("kodak_portra_400", "kodak_portra_endura", False),
    ("kodak_vision3_500t", "kodak_2383", False),
    ("fujifilm_provia_100f", "kodak_portra_endura", True),
    ("kodak_gold_200", "kodak_portra_endura", False),
]
# (name, input long side, Python upscale factor, Negs Image Size long side)
SIZES = [("640px", 640, 1.0, None), ("2016px", 2016, 1.0, None), ("1008px-upscaled-2x", 1008, 2.0, 2016)]


def tiles(np, long_side):
    width, height = long_side, round(long_side * 2 / 3)
    image = np.empty((height, width, 3), np.float32)
    edges = np.linspace(0, width, len(LEVELS) + 1).round().astype(int)
    for level, a, b in zip(LEVELS, edges[:-1], edges[1:]):
        image[:, a:b] = level
    return image, edges


def regions(edges, height, scale):
    """The middle of each tile, at the output's scale, clear of the borders
    where halation and DIR diffusion mix neighbouring tiles."""
    out = []
    for a, b in zip(edges[:-1], edges[1:]):
        a, b = int(a * scale), int(b * scale)
        margin = (b - a) // 6
        out.append((slice(int(height * scale) // 6, int(height * scale) * 5 // 6), slice(a + margin, b - margin)))
    return out


def stats(np, rgb, edges, height, scale):
    """Per tile and channel: mean and RMS in 8-bit sRGB steps, and the
    autocorrelation's half width in output pixels."""
    encoded = np.clip(rgb, 0, 1) * 255
    result = []
    for rows, cols in regions(edges, height, scale):
        patch = encoded[rows, cols]
        tile = {"mean": patch.mean(axis=(0, 1)).tolist(), "rms": patch.std(axis=(0, 1)).tolist()}
        lum = patch.mean(axis=2)
        lum = lum - lum.mean()
        power = np.abs(np.fft.fft2(lum)) ** 2
        acf = np.real(np.fft.ifft2(power))
        acf = np.fft.fftshift(acf / acf.flat[0])
        cy, cx = np.array(acf.shape) // 2
        y, x = np.indices(acf.shape)
        r = np.hypot(y - cy, x - cx).round().astype(int)
        radial = np.bincount(r.ravel(), acf.ravel()) / np.bincount(r.ravel())
        below = np.nonzero(radial < 0.5)[0]
        tile["acf_half_width_px"] = float(below[0] - 1 + (radial[below[0] - 1] - 0.5) /
                                          (radial[below[0] - 1] - radial[below[0]])) if len(below) else None
        tile["acf"] = radial[:12].tolist()
        freq_power = np.fft.fftshift(power)
        spectrum = np.bincount(r.ravel(), freq_power.ravel()) / np.bincount(r.ravel())
        n = min(acf.shape) // 2
        tile["psd"] = (spectrum[1:n] / lum.size).tolist()
        tile["psd_freq_cyc_per_px"] = (np.arange(1, n) / min(acf.shape)).tolist()
        result.append(tile)
    return result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--python-source", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/spektrafilm")
    parser.add_argument("--f64-binary", type=Path, default=ROOT / "target/release/spektrafilm-f64")
    parser.add_argument("--rust-data", type=Path, default=ROOT / "data")
    parser.add_argument("--release", type=Path, metavar="APP",
                        help="also measure a released Negs.app's GPU CLI, with its own data")
    parser.add_argument("--stock", action="append", help="limit to films containing this text")
    parser.add_argument("--view", type=int, metavar="WINDOW_PX", default=1512,
                        help="window width for the what-you-see comparison (0 skips it)")
    parser.add_argument("--view-full", type=int, default=6048, help="the photograph's long side in Negs")
    args = parser.parse_args(argv)
    out = args.output_dir.resolve()
    out.mkdir(parents=True, exist_ok=True)
    for variable, folder in [("NUMBA_CACHE_DIR", "numba"), ("MPLCONFIGDIR", "mpl"), ("XDG_CACHE_HOME", "xdg")]:
        os.environ.setdefault(variable, str(out / "cache" / folder))
    sys.dont_write_bytecode = True
    sys.path.insert(0, str(args.python_source.resolve() / "src"))
    import numpy as np
    import OpenImageIO as oiio
    from spektrafilm import init_params, digest_params
    from spektrafilm.runtime.pipeline import SimulationPipeline

    def write_tif(image, path):
        h, w, _ = image.shape
        buf = oiio.ImageBuf(oiio.ImageSpec(w, h, 3, "float"))
        buf.set_pixels(oiio.ROI(0, w, 0, h, 0, 1, 0, 3), image)
        if not buf.write(str(path)):
            raise RuntimeError(buf.geterror())

    def rust(binary, backend, tif, film, paper, scan, params, raw, data=None):
        (raw.parent / "params.json").write_text(json.dumps(params, indent=2) + "\n")
        env = {**os.environ}
        if backend:
            env["SPEKTRAFILM_BACKEND"] = backend
        start = time.perf_counter()
        done = subprocess.run([str(binary), "process", str(tif), "--data-dir", str(data or args.rust_data.resolve()),
                               "--output", str(raw.with_suffix(".png")), "--film", film, "--paper", paper,
                               "--params", str(raw.parent / "params.json"), "--raw-out", str(raw),
                               *(["--scan-film"] if scan else [])], env=env, capture_output=True, text=True)
        if done.returncode:
            raise RuntimeError(done.stderr[-800:])
        backend_line = next((l for l in done.stderr.splitlines() if l.startswith("Backend:")), "")
        return time.perf_counter() - start, backend_line.removeprefix("Backend: ").strip()

    report = {"levels": LEVELS, "cases": []}
    stocks = [s for s in STOCKS if not args.stock or any(t in s[0] for t in args.stock)]
    for film, paper, scan in stocks:
        for size_name, long_side, factor, negs_long in SIZES:
            directory = out / f"{film}-{size_name}"
            directory.mkdir(exist_ok=True)
            image, edges = tiles(np, long_side)
            height = image.shape[0]
            write_tif(image, directory / "input.tif")
            rust_params = {"camera": {"auto_exposure": False},
                           "film_render": {"glare": {"active": False}},
                           "print_render": {"glare": {"active": False}},
                           "io": {"scan_film": scan}}
            python_params = python_overrides(rust_params, "experimental")
            if factor != 1.0:
                python_params["io"]["upscale_factor"] = factor
                rust_params["io"]["photo_long_side_px"] = negs_long
            scale = factor
            case = {"film": film, "paper": paper, "scan_film": scan, "size": size_name,
                    "input_long_side": long_side, "output_long_side": round(long_side * scale),
                    "pixel_size_um": 35000 / (long_side * scale), "engines": {}}
            start = time.perf_counter()
            params = digest_params(apply_params(init_params(film, paper), python_params))
            py = np.asarray(SimulationPipeline(params).process(image.astype(np.float64)), dtype=np.float64)
            grain = params.film_render.grain
            case["python_grain_params"] = {"rms_granularity": list(map(float, grain.rms_granularity)),
                                           "uniformity": list(map(float, grain.uniformity)),
                                           "particle_scale_sublayers": list(map(float, grain.particle_scale_sublayers)),
                                           "blur": float(grain.blur), "mult_usm_sigma": float(grain.mult_usm_sigma),
                                           "mult_usm_amount": float(grain.mult_usm_amount)}
            case["engines"]["python"] = {"seconds": time.perf_counter() - start,
                                         "tiles": stats(np, py, edges, height, scale)}
            np.save(directory / "python.npy", py.astype(np.float32))
            engines = [("negs_gpu", args.binary, None, None), ("negs_f64_cpu", args.f64_binary, "cpu", None)]
            if args.release and factor == 1.0:  # releases before Image Size can't enlarge early
                engines.append(("release_gpu", args.release / "Contents/MacOS/spektrafilm", None,
                                args.release / "Contents/Resources/data"))
            for engine, binary, backend, data in engines:
                raw = directory / f"{engine}.raw"
                seconds, used = rust(binary, backend, directory / "input.tif", film, paper, scan, rust_params, raw, data)
                dtype = "=f8"  # --raw-out is f64 in every build
                rgb = np.fromfile(raw, dtype=dtype).astype(np.float64)
                rgb = rgb.reshape(py.shape)
                np.save(directory / f"{engine}.npy", rgb.astype(np.float32))
                raw.unlink()
                case["engines"][engine] = {"seconds": seconds, "backend": used,
                                           "tiles": stats(np, rgb, edges, height, scale)}
            report["cases"].append(case)
            summary = "  ".join(f"{e}: rms {np.mean([t['rms'] for t in v['tiles']]):.2f} "
                                f"acf {np.mean([t['acf_half_width_px'] for t in v['tiles']]):.2f}px"
                                for e, v in case["engines"].items())
            print(f"{film} {size_name}: {summary}", flush=True)
            (out / "grain.json").write_text(json.dumps(report) + "\n")

    if args.view:
        # What a person sees: Negs develops the full photograph and the canvas
        # shows it at the window's size (trilinear: an area average); the
        # Python GUI previews at 640 px and the viewer enlarges that.
        film, paper, scan = stocks[0]
        directory = out / f"{film}-view"
        directory.mkdir(exist_ok=True)
        full, edges = tiles(np, args.view_full)
        write_tif(full, directory / "input.tif")
        rust_params = {"camera": {"auto_exposure": False}, "film_render": {"glare": {"active": False}},
                       "print_render": {"glare": {"active": False}}, "io": {"scan_film": scan}}
        k = args.view_full // args.view

        def develop_and_show(binary, data, name):
            raw = directory / f"{name}.raw"
            rust(binary, None, directory / "input.tif", film, paper, scan, rust_params, raw, data)
            negs = np.fromfile(raw, dtype="=f8").reshape(full.shape).astype(np.float64)
            raw.unlink()
            shown = np.clip(negs, 0, 1)[: negs.shape[0] // k * k, : negs.shape[1] // k * k]
            return shown.reshape(shown.shape[0] // k, k, shown.shape[1] // k, k, 3).mean(axis=(1, 3))

        shown = develop_and_show(args.binary, None, "negs")
        release_shown = (develop_and_show(args.release / "Contents/MacOS/spektrafilm",
                                          args.release / "Contents/Resources/data", "release")
                         if args.release else None)
        preview, preview_edges = tiles(np, 640)
        params = digest_params(apply_params(init_params(film, paper), python_overrides(rust_params, "experimental")))
        py = np.clip(np.asarray(SimulationPipeline(params).process(preview.astype(np.float64))), 0, 1)
        from PIL import Image
        enlarged = np.stack([np.asarray(Image.fromarray(py[..., c].astype(np.float32)).resize(
            (shown.shape[1], shown.shape[0]), Image.BICUBIC)) for c in range(3)], axis=2).astype(np.float64)
        np.save(directory / "negs_shown.npy", shown.astype(np.float32))
        np.save(directory / "python_preview_shown.npy", enlarged.astype(np.float32))
        report["view"] = {"film": film, "window_px": shown.shape[1], "negs_long_side": args.view_full,
                          "python_preview_long_side": 640,
                          "negs_shown": stats(np, shown, edges, full.shape[0], 1 / k),
                          "python_preview_shown": stats(np, enlarged, preview_edges, preview.shape[0],
                                                        shown.shape[1] / 640)}
        if release_shown is not None:
            np.save(directory / "release_shown.npy", release_shown.astype(np.float32))
            report["view"]["release_shown"] = stats(np, release_shown, edges, full.shape[0], 1 / k)
        print("view: negs rms {:.2f}, python preview rms {:.2f}".format(
            np.mean([t["rms"] for t in report["view"]["negs_shown"]]),
            np.mean([t["rms"] for t in report["view"]["python_preview_shown"]])), flush=True)
        (out / "grain.json").write_text(json.dumps(report) + "\n")


if __name__ == "__main__":
    main()
