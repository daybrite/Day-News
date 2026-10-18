#!/usr/bin/env python3
"""Real Android pulls against dayscript/pull-refresh-seed.yaml, on an isolated emulator.

Run feed-refresh-server.py and adb reverse tcp:28762 tcp:28762 first. Do not run other
tests against the same server concurrently: HTTP request counts are the assertions.
"""
import argparse
import json
import re
import subprocess
import time
import urllib.request
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--adb", default="adb")
parser.add_argument("--serial", default="emulator-5554")
parser.add_argument("--screenshots", default="/tmp/day-news-android-refresh")
args = parser.parse_args()


def adb(*command):
    return subprocess.check_output([args.adb, "-s", args.serial, *command])


def hits():
    with urllib.request.urlopen("http://127.0.0.1:28762/stats", timeout=5) as response:
        return json.load(response)["hits"].get("/progress/known", 0)


sizes = re.findall(r"(?:Physical|Override) size: (\d+)x(\d+)", adb("shell", "wm", "size").decode())
width, height = map(int, sizes[-1])


def pull():
    adb("shell", "input", "swipe", str(width // 2), str(height * 3 // 10),
        str(width // 2), str(height * 8 // 10), "400")
    time.sleep(0.3)


def screenshot(name):
    directory = Path(args.screenshots)
    directory.mkdir(parents=True, exist_ok=True)
    (directory / f"{name}.png").write_bytes(adb("exec-out", "screencap", "-p"))


before = hits()
pull()
assert hits() == before + 1, "Native pull did not initiate a single refresh"
screenshot("active")
pull()
assert hits() == before + 1, "Repeated pull duplicated an in-flight refresh"
time.sleep(8)
screenshot("complete")
pull()
assert hits() == before + 2, "Completion did not allow another pull"
time.sleep(8)
print("PASS native Android pull, duplicate suppression, completion and subsequent pull")
