#!/usr/bin/env python3
"""Render neutral/HDR ramps, colour patches and exposure brackets; retain stages."""
import argparse
import copy
import json
import os
from pathlib import Path
import subprocess

os.environ.setdefault("MPLCONFIGDIR", "/tmp/spektrafilm-bw-mpl")
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
import OpenImageIO as oiio
from PIL import Image, ImageDraw, ImageFont, ImageOps

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "docs/bw-expansion-review"
FILM, PAPER = "kodak_trix400_d76_8min", "ilford_mgrc_deluxe_grade2"


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--binary", type=Path, default=ROOT / "target/release/spektrafilm-f64")
    ap.add_argument("--data-dir", type=Path, default=ROOT / "data")
    args = ap.parse_args()
    dest = OUT / "diagnostics"
    dest.mkdir(exist_ok=True)
    ev = np.linspace(-14, 10, 256)
    gray = .184*2**ev
    colors = np.array([[1,0,0],[0,1,0],[0,0,1],[1,1,0],[0,1,1],[1,0,1],[1,.5,0],[.5,0,1]])
    edge = np.array([[0]*3,[1e-12]*3,[1e-10]*3,[1e-8]*3,[1e3]*3,[1e6]*3])
    pixels = np.vstack([np.repeat(gray[:,None],3,axis=1),colors,edge]).astype(np.float32)[None]
    image = oiio.ImageBuf(oiio.ImageSpec(pixels.shape[1],1,3,"float"))
    image.set_pixels(oiio.ROI(0,pixels.shape[1],0,1,0,1,0,3),pixels)
    if not image.write(str(dest / "input.tif")):
        raise RuntimeError(image.geterror())
    base = json.loads((OUT / "controlled.json").read_text())
    base["io"].update(input_cctf_decoding=False, output_cctf_encoding=False)
    variants = {"trix-paper":(FILM,PAPER), "doublex-paper":("kodak_doublex",PAPER), "trix-2302":(FILM,"kodak_2302")}
    outputs = {}
    report = {"notes":"Linear sRGB, no effects/auto-exposure/gamut compression. Gray EV is relative to 0.184. Outside manufacturer ranges is model extrapolation.","cases":{}}
    for name,(film,paper) in variants.items():
        for lut in [False,True]:
            key = name+("-lut-on" if lut else "-lut-off")
            p = copy.deepcopy(base)
            p["settings"].update(use_enlarger_lut=lut,use_scanner_lut=lut)
            path = dest / (key+".json")
            path.write_text(json.dumps(p,indent=2)+"\n")
            env = {**os.environ,"SPEKTRAFILM_BACKEND":"cpu"}
            for stage, var in [("film-log-raw","SPEKTRAFILM_DUMP_FILM_LOG_RAW"),("film-density","SPEKTRAFILM_DUMP_FILM_DENSITY"),("paper-density","SPEKTRAFILM_DUMP_PRINT_DENSITY")]:
                env[var]=str(dest/(key+"-"+stage+".f64"))
            with (dest/(key+".log")).open("w") as log:
                subprocess.run([str(args.binary.resolve()),"process",str(dest/"input.tif"),"-o",str(dest/(key+".png")),"--film",film,"--paper",paper,"--params",str(path),"--data-dir",str(args.data_dir.resolve()),"--raw-out",str(dest/(key+"-rgb.f64"))],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
            arrays={stage:np.fromfile(dest/(key+"-"+stage+".f64"),dtype="<f8").reshape(-1,3) for stage in ["film-log-raw","film-density","paper-density","rgb"]}
            if not all(np.isfinite(a).all() for a in arrays.values()): raise ValueError("Nonfinite stage")
            rgb=arrays["rgb"]
            if np.any(np.diff(rgb[:256,1]) < -1e-10): raise ValueError("Nonmonotone output ramp")
            outputs[key]=arrays
            report["cases"][key]={"finite_all_stages":True,"monotone_gray_ramp":True,
                "black_rgb":rgb[-6].tolist(),"hdr_1e6_rgb":rgb[-1].tolist(),
                "colour_patch_rgb":rgb[256:264].tolist(),"max_neutral_channel_spread":float(np.ptp(rgb[:256],axis=1).max())}
    for name in variants:
        report["cases"][name+"-lut-on"]["max_rgb_difference_from_lut_off"]=float(np.max(abs(outputs[name+"-lut-on"]["rgb"]-outputs[name+"-lut-off"]["rgb"])))
    # Independent scalar reflection calculation from the retained density tap.
    paper_json=json.loads((args.data_dir/"profiles"/(PAPER+".json")).read_text())
    base_d=paper_json["data"]["base_density"][0][0]
    density=outputs["trix-paper-lut-off"]["paper-density"][:,0]
    expected_y=10**(-base_d-density)
    # sRGB output matrix Y row; chromatic adaptation has only sub-0.1% neutral error.
    rgb=outputs["trix-paper-lut-off"]["rgb"]
    y=rgb@np.array([.2126,.7152,.0722])
    report["reflection_reference_max_y_error"]=float(max(abs(y-expected_y)))
    if report["reflection_reference_max_y_error"] > 1e-3: raise ValueError("Reflection-density reference failed")
    fig,axes=plt.subplots(1,3,figsize=(14,4))
    for name in variants:
        a=outputs[name+"-lut-off"]
        axes[0].plot(ev,a["rgb"][:256,1],label=name)
        axes[1].plot(ev,a["film-density"][:256,0],label=name)
        axes[2].plot(ev,a["paper-density"][:256,0],label=name)
    for ax,title,ylabel in zip(axes,["Display tone (linear)","Negative density", "Print density"],["Linear green (neutral proxy)","Net diffuse density","Net print density"]):
        ax.set(title=title,xlabel="Input stops relative to 18.4% gray",ylabel=ylabel,xlim=(-10,8));ax.grid(alpha=.2)
    axes[0].legend(fontsize=7)
    fig.tight_layout();fig.savefig(OUT/"diagnostic-ramps.png",dpi=140);plt.close(fig)
    # Exposure changes alone: default print compensation remains on, explicitly.
    report["exposure_bracket"]={"ev":[-2,-1,0,1,2],"print_exposure_compensation":True,"auto_exposure":False}
    font=ImageFont.truetype("/System/Library/Fonts/Helvetica.ttc",16)
    canvas=Image.new("RGB",(1280,300),"#f5f3ef");draw=ImageDraw.Draw(canvas)
    for i,bracket_ev in enumerate([-2,-1,0,1,2]):
        p=json.loads((OUT/"controlled.json").read_text());p["camera"]["exposure_compensation_ev"]=bracket_ev
        path=dest/f"bracket-{bracket_ev:+d}.json";path.write_text(json.dumps(p,indent=2)+"\n")
        output=dest/f"bracket-{bracket_ev:+d}.png"
        with output.with_suffix(".log").open("w") as log:
            subprocess.run([str(args.binary.resolve()),"process",str(OUT/"inputs/astronaut.png"),"-o",str(output),"--film",FILM,"--paper",PAPER,"--params",str(path),"--data-dir",str(args.data_dir.resolve())],env={**os.environ,"SPEKTRAFILM_BACKEND":"cpu"},stdout=log,stderr=subprocess.STDOUT,check=True)
        canvas.paste(ImageOps.contain(Image.open(output),(256,256)),(i*256,36));draw.text((i*256+10,10),f"{bracket_ev:+d} EV",font=font,fill="#182728")
    canvas.save(OUT/"exposure-brackets.png")
    (dest/"report.json").write_text(json.dumps(report,indent=2)+"\n")
    print(json.dumps({"cases":len(report["cases"]),"reflection_reference_max_y_error":report["reflection_reference_max_y_error"]}))


if __name__=="__main__": main()
