#!/usr/bin/env python3
"""Exercise the native bridge with real renders, save, errors, and EOF cleanup.

Build with cargo build --release -p spektrafilm-native first.
Uses only the Python standard library. Set SPEKTRAFILM_BACKEND=cpu to force CPU.
"""
import argparse
import copy
import json
import os
import select
from pathlib import Path
import struct
import subprocess
import tempfile
import zlib

ROOT = Path(__file__).resolve().parents[1]


def png(path, width, height):
    def chunk(name, body):
        return struct.pack(">I", len(body)) + name + body + struct.pack(">I", zlib.crc32(name + body))
    pixels = b"".join(b"\0" + bytes(v for x in range(width) for v in (x * 255 // width, y * 255 // height, 100)) for y in range(height))
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">2I5B", width, height, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(pixels)) + chunk(b"IEND", b""))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--helper", type=Path, default=ROOT / "target/release/spektrafilm-native")
    parser.add_argument("--data-dir", type=Path, default=ROOT / "data")
    parser.add_argument("--exporter", type=Path, default=ROOT / "target/release/spektrafilm-f64")
    parser.add_argument("--fast-exporter", type=Path, default=ROOT / "target/release/spektrafilm")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="spektrafilm-native-test-") as work:
        work = Path(work)
        source, buffer, saved = work / "source.png", work / "preview.rgba", work / "saved.png"
        png(source, 32, 24)
        with (work / "engine.log").open("w+") as log:
            child = subprocess.Popen([args.helper.resolve(), "--data-dir", args.data_dir.resolve()], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log, text=True)
            sequence = 0

            def request(command, expect_error=False, **kwargs):
                nonlocal sequence
                sequence += 1
                child.stdin.write(json.dumps(dict(id=sequence, command=command, **kwargs)) + "\n")
                child.stdin.flush()
                assert select.select([child.stdout], [], [], 60)[0], "Engine response timed out"
                line = child.stdout.readline()
                if not line:
                    log.seek(0)
                    raise AssertionError(f"Engine stopped: {log.read()}")
                reply = json.loads(line)
                assert reply["id"] == sequence, reply
                assert ("error" in reply) == expect_error, reply
                return sequence, reply.get("result", reply.get("error"))

            try:
                _, catalog = request("catalog")
                assert catalog["protocol"] == 1
                params = catalog["defaults"]
                # Deterministic, small fixture still exercises the spectral pipeline.
                params["film_render"]["grain"]["active"] = False
                params["film_render"]["halation"]["active"] = False
                params["print_render"]["glare"]["active"] = False
                render_args = dict(input=str(source), film="kodak_gold_200", paper="kodak_portra_endura", params=params, limit_preview=False, preview_path=str(buffer))
                first_id, result = request("render", **render_args)
                assert (result["width"], result["height"]) == (32, 24), result
                first_bytes = buffer.read_bytes()
                assert len(first_bytes) == 32 * 24 * 4
                assert all(first_bytes[i] == 255 for i in range(3, len(first_bytes), 4))
                request("save", render_id=first_id, output=str(saved))
                assert saved.read_bytes().startswith(b"\x89PNG")
                params["camera"]["exposure_compensation_ev"] = 1.5
                params["io"]["rotate_cw"] = 1  # The exporter rotates; the app passes it to save.
                second_id, result = request("render", **render_args)
                assert buffer.read_bytes() != first_bytes, "Exposure must affect rendered pixels"
                request("save", expect_error=True, render_id=first_id, output=str(saved))
                request("save", render_id=second_id, output=str(saved), rotate_cw=1)
                assert struct.unpack(">II", saved.read_bytes()[16:24]) == (24, 32), "Saved rotation must match the UI"
                # The crop is in the unrotated photograph: the left half, then turned.
                request("save", render_id=second_id, output=str(saved), rotate_cw=1, crop=dict(x=0, y=0, width=0.5, height=1))
                assert struct.unpack(">II", saved.read_bytes()[16:24]) == (24, 16), "Saved crop must match the UI"
                request("save", expect_error=True, render_id=second_id, output=str(saved), rotate_cw=1, crop=dict(x=0.6, y=0, width=0.5, height=1))
                # Straightened: the sides round (25.6 × 19.2), then the quarter turn.
                request("save", render_id=second_id, output=str(saved), rotate_cw=1, crop=dict(x=0.1, y=0.1, width=0.8, height=0.8, angle=5))
                assert struct.unpack(">II", saved.read_bytes()[16:24]) == (19, 26), "Saved straightened crop must match the UI"
                request("save", expect_error=True, render_id=second_id, output=str(saved), rotate_cw=1, crop=dict(x=0, y=0, width=1, height=1, angle=60))
                request("render", expect_error=True, **(render_args | {"input": str(work / "missing.png")}))
                request("catalog")  # Errors leave the process usable.
                # Explicitly reopening a path must reload an externally changed file.
                png(source, 40, 30)
                render_args["source_generation"] = 1
                _, reopened = request("render", **render_args)
                assert (reopened["width"], reopened["height"]) == (40, 30)
                png(source, 32, 24)
                render_args["source_generation"] = 2
                slide_params = copy.deepcopy(params)
                slide_params["io"]["scan_film"] = True
                _, slide = request("render", **(render_args | {"film": "kodak_trix", "params": slide_params}))
                assert slide["width"] == 32
                params["io"]["upscale_factor"] = 0.5
                _, reduced = request("render", **render_args)
                assert (reduced["width"], reduced["height"]) == (16, 12)
                assert reduced["export_resolution"], reduced
                # The app exports Fast by saving a preview at export size: it
                # must be the file the Fast exporter writes, grain included.
                grain_params = copy.deepcopy(params)
                grain_params["film_render"]["grain"]["active"] = True
                grain_id, grain = request("render", **(render_args | {"params": grain_params}))
                assert grain["export_resolution"], grain
                crop = dict(x=0, y=0, width=0.5, height=1, angle=-2)
                preview_export, fast_export = work / "preview-export.tiff", work / "fast-export.tiff"
                request("save", render_id=grain_id, output=str(preview_export), crop=crop, rotate_cw=1)
                (work / "grain.json").write_text(json.dumps(grain_params))
                subprocess.run([args.fast_exporter.resolve(), "process", str(source), "-o", str(fast_export),
                                "--film", "kodak_gold_200", "--paper", "kodak_portra_endura", "--crop", "0,0,0.5,1,-2",
                                "--params", str(work / "grain.json"), "--data-dir", str(args.data_dir.resolve())],
                               env=dict(os.environ, SPEKTRAFILM_BACKEND=os.environ.get("SPEKTRAFILM_BACKEND", "wgpu")), check=True,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=60)
                assert preview_export.read_bytes() == fast_export.read_bytes(), "A saved preview at export size must equal the Fast export"
                export_params = work / "params.json"
                export_params.write_text(json.dumps(params))
                exported = work / "reference.png"
                subprocess.run([args.exporter.resolve(), "process", str(source), "-o", str(exported),
                                "--film", "kodak_gold_200", "--paper", "kodak_portra_endura",
                                "--params", str(export_params), "--data-dir", str(args.data_dir.resolve())],
                               env=dict(os.environ, SPEKTRAFILM_BACKEND="cpu"), check=True,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=60)
                assert struct.unpack(">II", exported.read_bytes()[16:24]) == (12, 16)
                subprocess.run([args.exporter.resolve(), "process", str(source), "-o", str(exported),
                                "--film", "kodak_gold_200", "--paper", "kodak_portra_endura", "--crop", "0,0,0.5,1,-2",
                                "--params", str(export_params), "--data-dir", str(args.data_dir.resolve())],
                               env=dict(os.environ, SPEKTRAFILM_BACKEND="cpu"), check=True,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=60)
                assert struct.unpack(">II", exported.read_bytes()[16:24]) == (12, 8), "Exported crop must match the UI"
                child.stdin.close()
                assert child.wait(timeout=10) == 0, "EOF should cleanly release the engine"
                print(json.dumps({"passed": True, "requests": sequence, "backend": result["backend"], "profiles": len(catalog["profiles"]), "f64_export": True, "fast_export_from_preview": True}))
            finally:
                if child.poll() is None:
                    child.kill()
                    child.wait()


if __name__ == "__main__":
    main()
