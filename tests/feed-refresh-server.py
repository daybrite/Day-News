"""Synthetic conditional-feed and icon fixtures; no publisher traffic.

Run this server, then dayscript/feed-refresh.yaml with a fresh DAY_NEWS_DATA_DIR.
GET /stats reports validators, 200/304 totals, icon hits and maximum concurrent feeds.
"""
import json
import struct
import threading
import time
import zlib
from collections import Counter
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PORT = 28762
BASE = f"http://127.0.0.1:{PORT}"
MODIFIED = "Wed, 30 Sep 2026 10:00:00 GMT"
LOCK = threading.Lock()
STATS = {"active": 0, "max_active": 0, "modified": 0, "unchanged": 0, "invalid_conditions": 0}
HITS = Counter()


def png(color):
    def chunk(kind, data):
        return struct.pack("!I", len(data)) + kind + data + struct.pack("!I", zlib.crc32(kind + data))
    pixels = b"".join(b"\0" + bytes(color) * 32 for _ in range(32))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack("!2I5B", 32, 32, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(pixels)) + chunk(b"IEND", b""))


class Handler(BaseHTTPRequestHandler):
    def send(self, status, body, content_type, headers=()):
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        for key, value in headers:
            self.send_header(key, value)
        self.end_headers()
        if status != 304:
            try:
                self.wfile.write(body)
            except (BrokenPipeError, ConnectionResetError):
                pass

    def do_GET(self):
        with LOCK:
            HITS[self.path] += 1
        if self.path == "/stats":
            with LOCK:
                body = json.dumps(dict(STATS, hits=dict(HITS))).encode()
            self.send(200, body, "application/json")
        elif self.path.startswith("/progress/"):
            mode = self.path.rsplit("/", 1)[1]
            # Delay headers to exercise connecting, then drip a valid feed. The second feed
            # deliberately has no Content-Length, so it must keep spinning throughout.
            time.sleep(2)
            body = json.dumps({"version": "https://jsonfeed.org/version/1.1",
                "title": "Known length fixture" if mode == "known" else "Unknown length fixture",
                "icon": f"{BASE}/icon/{1 if mode == 'known' else 2}.png",
                "items": [{"id": mode, "title": "Synthetic progress article",
                    "content_text": "Fixture paragraph. " * 1024}]}).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/feed+json")
            if mode == "known": self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            try:
                size = (len(body) + 15) // 16
                for start in range(0, len(body), size):
                    self.wfile.write(body[start:start+size])
                    self.wfile.flush()
                    time.sleep(0.25)
            except (BrokenPipeError, ConnectionResetError):
                pass
        elif self.path.startswith("/feed/"):
            number = int(self.path.rsplit("/", 1)[1])
            etag = f'W/"fixture-{number}"'
            with LOCK:
                STATS["active"] += 1
                STATS["max_active"] = max(STATS["active"], STATS["max_active"])
            time.sleep(2)
            condition = self.headers.get("If-None-Match")
            unchanged = condition == etag and self.headers.get("If-Modified-Since") == MODIFIED
            with LOCK:
                STATS["active"] -= 1
                STATS["unchanged" if unchanged else "modified"] += 1
                STATS["invalid_conditions"] += int(condition is not None and not unchanged)
            feed = {"version": "https://jsonfeed.org/version/1.1", "title": f"Fixture {number}",
                    "home_page_url": f"{BASE}/site/{number}", "items": [
                        {"id": f"fixture-{number}", "title": f"Fixture article {number}",
                         "content_html": "<p>Stable original body.</p>",
                         "date_published": f"2026-09-{number:02}T12:00:00Z"}]}
            if number == 1:
                feed["icon"] = f"{BASE}/icon/1.png"
            self.send(304 if unchanged else 200, json.dumps(feed).encode(), "application/feed+json",
                      [("ETag", etag), ("Last-Modified", MODIFIED)])
        elif self.path.startswith("/site/"):
            number = int(self.path.rsplit("/", 1)[1])
            link = ("<link rel='manifest' href='/manifest.json'>" if number == 3
                    else f"<link rel='icon' sizes='32x32' href='../icon/{number}.png'>" if number == 2
                    else "")
            self.send(200, f"<html><head>{link}</head><body>Fixture home</body></html>".encode(), "text/html")
        elif self.path == "/manifest.json":
            self.send(200, b'{"icons":[{"src":"/icon/3.png","sizes":"32x32"}]}', "application/json")
        elif self.path.startswith("/icon/") or self.path == "/favicon.ico":
            number = int(self.path.split("/")[-1].split(".")[0]) if self.path.startswith("/icon/") else 4
            self.send(200, png({1: (240, 80, 40), 2: (50, 180, 80), 3: (100, 80, 230), 4: (240, 170, 20)}[number]), "image/png")
        else:
            self.send(404, b"Missing fixture", "text/plain")


if __name__ == "__main__":
    print(f"Feed refresh fixture server: {BASE}", flush=True)
    ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
