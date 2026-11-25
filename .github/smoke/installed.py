import json
import atexit
import os
from pathlib import Path
import queue
import subprocess
import threading
import time
import urllib.request
import uuid

import websocket
from pywinauto import Desktop

tmp = Path(os.environ["RUNNER_TEMP"])
install = tmp / "Cutroom Release"
sample = tmp / "한글 배포 샘플"
media = sample / "media"
media.mkdir(parents=True)
asset_id, scene_id = str(uuid.uuid4()), str(uuid.uuid4())
movie = media / "sample.mp4"
subprocess.run([install / "media-tools/ffmpeg.exe", "-v", "error", "-f", "lavfi", "-i", "testsrc2=s=320x180:r=24", "-t", "2", "-c:v", "libx264", "-pix_fmt", "yuv420p", str(movie)], check=True)
duration = subprocess.check_output([install / "media-tools/ffprobe.exe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", str(movie)], text=True)
assert float(duration) == 2
project = sample / "project.json"
title, scene_name = "컷룸 배포 확인", "Sample scene"
project.write_text(json.dumps({"title": title, "mediaTree": True, "assets": [
    {"id":asset_id, "name":"sample.mp4", "extension":"mp4", "kind":"video", "hasAudio":False, "durationMs":2000, "projectFile":"media/sample.mp4"}
], "scenes":[{"id":scene_id, "name":scene_name, "revision":1, "durationMs":2000, "text":"Sample", "caption":"Sample caption", "visual":{"assetId":asset_id,"inMs":0},"audio":None}]}), encoding="utf-8")

def until(fn, label, seconds=60):
    end = time.monotonic() + seconds
    last = None
    while time.monotonic() < end:
        try:
            value = fn()
            if value:
                return value
        except Exception as ex:
            last = ex
        time.sleep(0.45)
    raise RuntimeError(f"Timed out: {label}, {last}")

# The packaged WebView and native picker both belong to the installed editor.
env = dict(os.environ, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9327 --force-renderer-accessibility")
app = subprocess.Popen([install / "tauri-app.exe"], env=env)
def diagnostic():
    print("Editor exit status:", app.poll(), "Bridge exists:", (Path(os.environ["LOCALAPPDATA"])/"com.namuk.tauri-app/editor-control.json").exists())
    print("Desktop windows:", [(w.process_id(), w.class_name(), w.window_text()[:100]) for w in Desktop(backend="win32").windows()][:40])
    subprocess.run(["powershell","-NoProfile","-Command","Get-Process tauri-app,msedgewebview2,explorer -ErrorAction SilentlyContinue | Select-Object Name,Id,SessionId | Format-Table; Get-ChildItem '${env:ProgramFiles(x86)}/Microsoft/EdgeWebView/Application' -Name -ErrorAction SilentlyContinue; Get-NetTCPConnection -State Listen -LocalPort 9327 -ErrorAction SilentlyContinue | Select-Object LocalAddress,OwningProcess"])
atexit.register(diagnostic)
def attach():
    urls = json.load(urllib.request.build_opener(urllib.request.ProxyHandler({})).open("http://127.0.0.1:9327/json/list",timeout=4))
    page = next(t for t in urls if t["type"]=="page" and "tauri.localhost" in t["url"])
    return websocket.create_connection(page["webSocketDebuggerUrl"],suppress_origin=True,timeout=8)
ws = None
try:
    ws = until(attach, "desktop WebView debugging channel", 10)
except RuntimeError as e:
    print("Using Windows accessibility for the installed window.",e)
serial=0
def js(expr):
    global serial
    serial+=1
    ws.send(json.dumps({"id":serial,"method":"Runtime.evaluate","params":{"expression":expr,"returnByValue":True}}))
    while True:
        msg=json.loads(ws.recv())
        if msg.get("id")==serial:
            return msg.get("result",{}).get("result",{}).get("value")

if ws:
    until(lambda: js("Array.from(document.querySelectorAll('button')).some(b=>b.innerText.trim()==='기존 프로젝트 열기')"),"Home")
    js("Array.from(document.querySelectorAll('button')).find(b=>b.innerText.trim()==='기존 프로젝트 열기').click()")
else:
    diagnostic()
    window=Desktop(backend="uia").window(process=app.pid,title="컷룸")
    button=window.child_window(title="기존 프로젝트 열기",control_type="Button")
    button.wait("exists",timeout=35)
    button.invoke()
print("PASS installed Home rendered and existing-project button invoked")
def picker():
    for dlg in Desktop(backend="win32").windows(class_name="#32770",visible_only=True):
        if dlg.process_id()==app.pid and "project.json" in dlg.window_text():
            return dlg
dlg=until(picker, "native existing-project file picker")
edit=until(lambda: next((e for e in dlg.descendants(class_name="Edit") if e.is_visible() and e.is_enabled()),None), "picker filename")
edit.set_edit_text(str(project))
open_button = next(b for b in dlg.descendants(class_name="Button") if b.control_id()==1)
open_button.click()
if ws:
    until(lambda: js("location.search.includes('view=editor') && document.querySelector('.studio input[aria-label=\"프로젝트 이름\"]')?.value === '컷룸 배포 확인'"), "editor displaying the selected project's name")
else:
    until(lambda: window.child_window(title="프로젝트 이름",control_type="Edit").get_value()==title, "selected project's title")
print("PASS native file picker selected the sample and the editor rendered")

# Use Codex itself, in an empty working directory, with the installer's default registration.
workspace=tmp/"empty-codex-workspace"
workspace.mkdir()
client=tmp/"codex-test/node_modules/.bin/codex.cmd"
config=json.loads(subprocess.check_output([str(client),"mcp","get","cutroom","--json"],cwd=workspace,encoding="utf-8"))
assert Path(config["transport"]["command"]).resolve()==(install/"mcp/cutroom-node.exe").resolve()
print("PASS Codex loaded installer-provided configuration")
proc=subprocess.Popen([str(client),"app-server"],cwd=workspace,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=None,text=True,encoding="utf-8")
inbox=queue.Queue()
def pump():
    for line in proc.stdout:
        try: inbox.put(json.loads(line))
        except ValueError: pass
    inbox.put(None)
threading.Thread(target=pump,daemon=True).start()
counter=0
def send(obj):
    proc.stdin.write(json.dumps(obj)+"\n")
    proc.stdin.flush()
def rpc(method, params):
    global counter
    counter+=1
    send({"id":counter,"method":method,"params":params})
    deadline=time.monotonic()+60
    while True:
        msg=inbox.get(timeout=max(0.01,deadline-time.monotonic()))
        if msg is None: raise RuntimeError("Codex app-server exited")
        if msg.get("id")==counter and "method" not in msg:
            if "error" in msg: raise RuntimeError(str(msg["error"]))
            return msg["result"]
rpc("initialize", {"clientInfo":{"name":"release-smoke","version":"1"},"capabilities":{"experimentalApi":True}})
send({"method":"initialized"})
thr=rpc("thread/start", {"cwd":str(workspace),"ephemeral":True,"approvalPolicy":"never"})["thread"]["id"]
def call(name,args):
    data=rpc("mcpServer/tool/call", {"threadId":thr,"server":"cutroom","tool":name,"arguments":args})
    if data.get("isError"): raise RuntimeError(f"{name} {data}")
    return data["content"][0]["text"]
guide=call("workspace_guide",{})
assert len(guide)>100
timeline=json.loads(call("timeline",{}))
assert timeline["title"]==title and len(timeline["scenes"])==1
assert timeline["scenes"][0]["id"]==scene_id and timeline["scenes"][0]["durationMs"]==2000
assert Path(timeline["mediaDirectory"]).resolve()==media.resolve()
asset=json.loads(call("get_asset",{"assetId":asset_id}))
assert asset["available"] and asset["durationMs"]==2000 and Path(asset["path"]).resolve()==movie.resolve()
print("PASS real Codex app-server -> installed Cutroom: workspace guide, correct sample title, scene ID, 2-second timeline and available sample video")
if ws: ws.close()
atexit.unregister(diagnostic)
proc.terminate()
app.terminate()
