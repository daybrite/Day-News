"""Synthetic RSS/publication fixture for dayscript/reader-view.yaml. No external network."""
import json
import time
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 28761
BASE = f"http://127.0.0.1:{PORT}"


class Handler(BaseHTTPRequestHandler):
    def do_OPTIONS(self):
        self.send_response(204)
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, OPTIONS")
        self.end_headers()

    def do_GET(self):
        if self.path in ("/feed.json", "/long-feed.json"):
            body = json.dumps({
                "version": "https://jsonfeed.org/version/1.1",
                "title": "Reader View Fixtures",
                "items": [
                    {"id": name, "url": f"{BASE}/{name}", "title": name,
                     "date_published": f"2026-09-{30-i:02}T12:00:00Z",
                     "content_html": f"<p>RSS excerpt for {name}.</p><a href='/reference'>Fixture reference</a>" + ("<p>Long synthetic RSS preview paragraph for visibility testing.</p>" * 100 if self.path == "/long-feed.json" else "")}
                    for i, name in enumerate(["complete", "slow", "denied"] if self.path == "/feed.json" else ["slow"])
                ],
            }).encode()
            content_type = "application/feed+json"
            status = 200
        else:
            if self.path == "/slow":
                time.sleep(4)
            paragraph = ("Synthetic full article: a telescope collects distant light and reveals "
                         "details that our eyes cannot see alone. Observers repeat their measurements "
                         "on clear nights and compare the results carefully. ")
            body = ("<html><head><title>Complete telescope article</title></head><body>"
                    "<nav>Fixture navigation</nav><article><h1>Complete telescope article</h1>"
                    f"<p>{paragraph * 12}</p><p id='full-text-fixture'>Full publication content.</p>"
                    "<a href='/reference'>Reference</a><script>window.publisherScriptRan=true</script>"
                    "</article></body></html>").encode()
            content_type = "text/html; charset=utf-8"
            status = 403 if self.path == "/denied" else 200
        self.send_response(status)
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        try:
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            pass  # Cancellation is exercised by the regression script.


if __name__ == "__main__":
    print(f"Reader View fixture server: {BASE}", flush=True)
    ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
