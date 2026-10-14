#!/usr/bin/env python3
"""Launch regression on a booted simulator: UIKit scene, Metal, UI, save/open and playback.

Usage: python3 packaging/ios/smoke.py DEVICE [--set DEVICE_SET] [--app PATH]
The app must be a Debug build (its control channel is absent in Release).
"""
import argparse
import json
from pathlib import Path
import socket
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("device")
    parser.add_argument("--set", dest="device_set")
    parser.add_argument("--app", default="target/ios-xcode/Build/Products/Debug-iphonesimulator/EffectCraft.app")
    args = parser.parse_args()
    sim = ["xcrun", "simctl"] + (["--set", args.device_set] if args.device_set else [])
    bundle = "com.artcraft.effectcraft.ios"
    subprocess.run(sim + ["install", args.device, args.app], check=True)
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    subprocess.run(sim + ["launch", "--terminate-running-process", args.device, bundle, f"--control={port}"], check=True)

    def request(method, params=None):
        with socket.create_connection(("127.0.0.1", port), timeout=2) as sock:
            sock.settimeout(60)
            sock.sendall((json.dumps({"method": method, "params": params or {}}) + "\n").encode())
            with sock.makefile("rb") as stream:
                result = json.loads(stream.readline(8 * 1024 * 1024))
        if not result.get("ok"):
            raise RuntimeError(f"{method}: {result}")
        return result.get("result")

    def wait_for(check, label):
        # A cold simulator's first Metal pipeline compilation can take about a minute.
        until = time.monotonic() + 120
        last = None
        while time.monotonic() < until:
            try:
                result = check()
                if result:
                    return result
            except (OSError, RuntimeError) as exc:
                last = exc
            time.sleep(0.25)
        raise RuntimeError(f"Timed out waiting for {label}: {last}")

    def command(command, params=None):
        return request("engine.execute", {"command": command, "params": params or {}})

    # This failed with SIGTRAP before the scene manifest, and silently exited before the
    # adapter-limit fix. An answer requires the real app's UI thread to be drawing.
    state = wait_for(lambda: request("ui.inspect"), "editor startup")
    print("Editor ready:", state.get("size", state.get("window")))
    layouts = wait_for(lambda: request("ui.elements", {"prefix": "ios.layout."}), "safe layout")
    chrome = next(item["rect"] for item in layouts if item["id"] == "ios.layout.chrome")
    menu = wait_for(lambda: request("ui.elements", {"prefix": "menu.File"}), "File menu")[0]["rect"]
    if not (chrome[0] <= menu[0] and chrome[1] <= menu[1]
            and menu[0] + menu[2] <= chrome[0] + chrome[2]
            and menu[1] + menu[3] <= chrome[1] + chrome[3]):
        raise RuntimeError("File menu is outside the navigation safe bounds")
    wait_for(lambda: request("ui.elements", {"prefix": "menu.File"}), "safe-area navigation")
    request("ui.click", {"id": "menu.File"})
    time.sleep(0.3)
    request("ui.key", {"key": "Escape"})
    command("file.newProject")
    command("comp.new", {"name": "iOS Smoke", "width": 320, "height": 180, "duration": 1, "frameRate": 30})
    command("layer.newSolid", {"color": "#3080ff"})
    # Exercise actual shortcut dispatch: Command Undo/Redo; Control must remain distinct.
    def layer_count():
        return next(item["layers"] for item in command("project.summary")["items"] if item.get("type") == "Composition")

    layers = layer_count()
    request("ui.key", {"key": "Ctrl+Z"})
    time.sleep(0.3)
    if layer_count() != layers:
        raise RuntimeError("Control+Z incorrectly triggered Command Undo")
    request("ui.key", {"key": "Cmd+Z"})
    wait_for(lambda: layer_count() == layers - 1, "Command Undo")
    request("ui.key", {"key": "Cmd+Shift+Z"})
    wait_for(lambda: layer_count() == layers, "Command Redo")
    data = subprocess.check_output(sim + ["get_app_container", args.device, bundle, "data"], text=True).strip()
    saved = Path(data) / "Documents" / f"iOS Smoke-{time.time_ns()}.ecproj"
    command("file.saveAs", {"path": str(saved)})
    wait_for(lambda: saved.exists() and saved.stat().st_size > 0, "saved project")
    project = json.loads(saved.read_text())
    if not project:
        raise RuntimeError("Saved project is empty")
    command("file.open", {"path": str(saved)})
    request("ui.playback", {"action": "play"})
    time.sleep(1.5)
    request("ui.playback", {"action": "stop"})
    request("ui.inspect")
    frame = Path(data) / "Documents" / "Exports" / f"Smoke-frame-{time.time_ns()}.png"
    frame.parent.mkdir(exist_ok=True)
    command("comp.saveFrameAs", {"path": str(frame)})
    wait_for(lambda: frame.exists(), "PNG export")
    if frame.read_bytes()[:8] != b"\x89PNG\r\n\x1a\n":
        raise RuntimeError("PNG export is invalid")
    movie = frame.with_suffix(".mp4")
    command("renderQueue.add", {"format": "h264", "output": str(movie), "timeSpan": "custom", "start": 0.0, "end": 0.1, "audio": "off"})
    command("renderQueue.render", {"wait": True})
    wait_for(lambda: movie.exists() and movie.stat().st_size > 0, "MP4 export")
    probe = subprocess.check_output(["ffprobe", "-v", "error", "-show_entries", "stream=codec_name,width,height", "-of", "json", str(movie)], text=True)
    streams = json.loads(probe)["streams"]
    if not any(stream.get("codec_name") == "h264" and stream.get("width") == 320 and stream.get("height") == 180 for stream in streams):
        raise RuntimeError("MP4 export has incorrect media metadata")
    queued = len(command("renderQueue.list")["items"])
    request("ui.key", {"key": "Cmd+M"})
    wait_for(lambda: len(command("renderQueue.list")["items"]) == queued + 1, "Command Add to Render Queue")
    saves = Path(data) / "Documents" / "Saved"
    before = set(saves.glob("*/*.ecproj"))
    request("ui.key", {"key": "Cmd+S"})
    wait_for(lambda: set(saves.glob("*/*.ecproj")) - before, "Command Save local staging")
    print("PASS: scene startup, safe navigation, Command Undo/Redo/Save/Render Queue, Documents save/open, playback, PNG and H.264 MP4 export")


if __name__ == "__main__":
    main()
