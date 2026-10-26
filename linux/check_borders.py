#!/usr/bin/env python3
"""Real GTK numeric/color/undo/session checks for presentation borders."""
import json
import os
from pathlib import Path
import subprocess
import time
import gi
from desktop_test import prepare_window
gi.require_version("Atspi", "2.0")
gi.require_version("GdkPixbuf", "2.0")
from gi.repository import Atspi, GdkPixbuf

root=Path(__file__).resolve().parent.parent
out=root/"target/linux-validation/border-controls"
out.mkdir(parents=True,exist_ok=True)
env=dict(os.environ, NEGS_UI_ONLY="1",GTK_A11Y="atspi",NEGS_PROFILE_DIR=str(out/"profile"),NEGS_UI_SCENE="post",NEGS_QUIT_AFTER_MS="18000")
Atspi.init()
def wait(fn,label):
    deadline=time.monotonic()+6
    while time.monotonic()<deadline:
        value=fn()
        if value:return value
        time.sleep(.03)
    raise AssertionError(label)
def tree(node):
    queue=[node]
    while queue:
        n=queue.pop(0);yield n
        queue.extend(c for i in range(n.get_child_count()) if (c:=n.get_child_at_index(i)) is not None)
def launch(fresh):
    log=(out/"app.log").open("a")
    p=subprocess.Popen([str(root/"target/linux-ui/release/negs")]+(["--demo","--fresh"] if fresh else []),env=env,stdout=log,stderr=log)
    prepare_window(p)
    desktop=Atspi.get_desktop(0)
    def find():
        return next((desktop.get_child_at_index(i) for i in range(desktop.get_child_count()) if desktop.get_child_at_index(i).get_process_id()==p.pid),None)
    app=wait(find,"Native app not registered")
    wait(lambda:any(n.get_name()=="Mat thickness value" for n in tree(app)),"Border controls not visible")
    return p,app,log
def close(p):
    subprocess.run(["hyprctl","dispatch",f'hl.dsp.window.close({{window="pid:{p.pid}"}})'],check=True,stdout=subprocess.DEVNULL)
    p.wait(timeout=4)
def named(app,name):return next(n for n in tree(app) if n.get_name()==name)

checks=[]
p,app,log=launch(True)
try:
    thickness=named(app,"Mat thickness value").get_value_iface()
    before=thickness.get_current_value()
    assert thickness.set_current_value(8.)
    wait(lambda:thickness.get_current_value()==8.,"Mat thickness did not update")
    assert named(app,"Undo (Ctrl+Z)").get_action_iface().do_action(0)
    wait(lambda:thickness.get_current_value()==before,"Border width did not undo")
    assert named(app,"Redo (Ctrl+Shift+Z)").get_action_iface().do_action(0)
    wait(lambda:thickness.get_current_value()==8.,"Border width did not redo")
    line=named(app,"Line thickness value").get_value_iface()
    assert line.set_current_value(.8)
    wait(lambda:abs(line.get_current_value()-.8)<1e-9,"Line thickness did not update")
    checks.append("Native mat/line numeric controls, undo and redo")
    subprocess.run(["hyprctl","dispatch",f'hl.dsp.focus({{window="pid:{p.pid}"}})'],check=True,stdout=subprocess.DEVNULL)
    entry=named(app,"Mat color hex")
    assert entry.get_editable_text_iface().set_text_contents("#233445")
    assert named(app,"Apply Mat color").get_action_iface().do_action(0)
    time.sleep(.3)
    clients=json.loads(subprocess.check_output(["hyprctl","-j","clients"],text=True));client=next(c for c in clients if c["pid"]==p.pid)
    x,y=client["at"];w,h=client["size"]
    subprocess.run(["grim","-g",f"{x},{y} {w}x{h}",str(out/"configured-keyline.png")],check=True)
    pixels=GdkPixbuf.Pixbuf.new_from_file(str(out/"configured-keyline.png")).read_pixel_bytes().get_data()
    assert pixels.count(bytes([35,52,69]))>1000,"Chosen border color did not reach the rendered canvas"
    checks.append("Native hex entry activation updates actual compositor border pixels")
    close(p)
    border=json.loads((out/"profile/session.json").read_text())["state"]["border"]
    assert border["width"]==.08 and border["line"]==.008 and border["color"]=="#233445"
    checks.append("Border thickness and color persist in the UI session")
finally:
    if p.poll() is None:p.terminate();p.wait(timeout=3)
    log.close()
p,app,log=launch(False)
try:
    assert named(app,"Mat thickness value").get_value_iface().get_current_value()==8.
    assert Atspi.Text.get_text(named(app,"Mat color hex"),0,-1)=="#233445"
    checks.append("Restored native controls show the saved settings")
    close(p)
finally:
    if p.poll() is None:p.terminate();p.wait(timeout=3)
    log.close()
(out/"result.json").write_text(json.dumps({"status":"PASS","checks":checks},indent=2)+"\n")
print("PASS:",*checks,sep="\n  ")
