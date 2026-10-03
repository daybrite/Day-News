"""Synthetic feed discovery origin. Records requests to verify that discovery does not double-fetch."""
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
PORT = 28763
BASE = f"http://127.0.0.1:{PORT}"
LOG = Path("/private/tmp/news-discovery-requests.jsonl")
class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        with LOG.open("a") as log:
            log.write(json.dumps(self.path) + "\n")
        headers = []
        if self.path.endswith(".xml"):
            title = self.path.strip("/").removesuffix(".xml").title()
            body = f'<rss version="2.0"><channel><title>Fixture {title}</title><link>{BASE}/</link><description>Discovery fixture</description><item><guid>{title}</guid><title>Fixture {title} article</title><link>{BASE}/article</link><description>Fixture body</description></item></channel></rss>'.encode()
            if self.path == "/empty.xml":
                body = f'<rss version="2.0"><channel><title>Fixture Empty</title><link>{BASE}/</link><description>Empty fixture</description></channel></rss>'.encode()
            kind = "application/rss+xml"
            headers.append(("ETag", '"fixture-v1"'))
        else:
            pages = {
                "/site": "<base href='/feeds/'><link rel='alternate' type='application/atom+xml' href='single.xml' title='Single fixture'>",
                "/multi": "<link rel='alternate' type='application/rss+xml' href='/alpha.xml' title='Fixture Alpha'><link rel='alternate' type='application/rss+xml' href='/beta.xml' title='Fixture Beta'>",
                "/header": "Fixture header advertised feed",
                "/missing": "Fixture page with no feed",
            }
            if self.path == "/header":
                headers.append(("Link", '</header.xml>; rel="alternate"; type="application/rss+xml"'))
            body = ("<html><head>" + pages.get(self.path, "Fixture page") + "</head><body>Fixture</body></html>").encode()
            kind = "text/html"
        self.send_response(200)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        for key,value in headers:
            self.send_header(key,value)
        self.end_headers()
        self.wfile.write(body)
if __name__ == "__main__":
    LOG.write_text("")
    print(f"Discovery fixture: {BASE}", flush=True)
    ThreadingHTTPServer(("127.0.0.1",PORT),Handler).serve_forever()
