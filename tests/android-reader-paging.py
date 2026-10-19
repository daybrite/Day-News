#!/usr/bin/env python3
"""Real Android paging regression. Requires seed-demo in an isolated running app."""
import argparse
import json
import os
import subprocess
import time
from pathlib import Path
import xml.etree.ElementTree as ET

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--adb', required=True)
parser.add_argument('--day', required=True)
parser.add_argument('--serial', default='emulator-5554')
args = parser.parse_args()

def adb(*commands):
    return subprocess.check_output([args.adb, '-s', args.serial, *commands])

def drive(*steps):
    result = subprocess.run([args.day, 'drive', '-t', 'android-mdc', '--steps-json', json.dumps(steps)], capture_output=True, text=True, env={**os.environ, 'ANDROID_SERIAL': args.serial})
    assert '{' in result.stdout, (result.stdout, result.stderr)
    report = json.loads(result.stdout[result.stdout.index('{'):])
    for step in report.get('steps', []):
        if 'screenshot' in step:
            step['screenshot'].pop('pngBase64', None)
    assert result.returncode == 0 and report['failed'] == 0, report

def evaluate(script, text):
    drive({'web_eval': {'id': 'reader-web', 'script': script, 'text': text}})

width, height = map(int, adb('shell', 'wm', 'size').decode().split()[-1].split('x'))
def swipe(start, end, milliseconds=700):
    adb('shell', 'input', 'swipe', str(int(width*start)), str(int(height*.5)), str(int(width*end)), str(int(height*.5)), str(milliseconds))
    time.sleep(1)

first = 'How monarch butterflies find their way to Mexico'
second = "What a honeybee's dance tells the hive"
title = 'document.getElementById("reader-title").textContent'
drive({'navigate': {'route': 'feed:5756297993796914528'}}, {'wait_for': {'id': 'timeline'}}, {'select': {'id': 'timeline', 'index': 0}}, {'pause': {'secs': 2}})
evaluate(title, first)
evaluate('window.pagerRetention="retained"; window.scrollTo(0,100); "ready"', 'ready')
# A short slow drag cancels. It must not replace the document or lose its scroll.
swipe(.65,.60,800)
evaluate('window.pagerRetention + ":" + Math.round(scrollY)', 'retained:100')
for _ in range(5):
    swipe(.85,.15)
    evaluate(title,second)
    swipe(.15,.85)
    evaluate(title,first)
    evaluate('window.pagerRetention + ":" + Math.round(scrollY)', 'retained:100')
# First-page boundary does not wrap.
swipe(.15,.85)
evaluate(title,first)
# Walk far enough to recycle every slot, then reverse through the same reading order.
fixture = Path(__file__).resolve().parents[1] / 'resource/assets/demo/en/field-notes.xml'
titles = [item.findtext('title') for item in ET.parse(fixture).findall('./channel/item')]
for expected in titles[1:]:
    swipe(.85,.15)
    evaluate(title, expected)
swipe(.85,.15)
evaluate(title, titles[-1])
for expected in reversed(titles[:-1]):
    swipe(.15,.85)
    evaluate(title, expected)
drive({'screenshot': {'name': 'native-pager-android-retained'}})
print('PASS native Android paging: cancellation, repeated reversal, document/scroll retention, recycled slots, both boundaries')
