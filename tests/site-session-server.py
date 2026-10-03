"""Synthetic login/storage fixture. Credentials are fixtures, never user accounts."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
PORT = 28764
BASE = f"http://127.0.0.1:{PORT}"
class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        path = self.path.split('?')[0]
        authenticated = 'session=fixture' in self.headers.get('Cookie', '')
        cookie = None
        if path == '/feed.json':
            body = json.dumps({'version':'https://jsonfeed.org/version/1.1','title':'Session Fixture','home_page_url':BASE+'/site','items':[{'id':'protected','title':'Protected fixture article','url':BASE+'/protected','date_published':'2026-10-03T12:00:00Z','content_html':'<p>Public synthetic preview.</p>'}]}).encode()
            mime = 'application/feed+json';status=200
        elif path == '/protected':
            paragraph = 'Authenticated synthetic publication text about careful observations and reliable experiments. '*40
            body = f'<html><head><title>Protected fixture article</title></head><body><article><p>{paragraph}</p><p>Authenticated full article fixture.</p></article></body></html>'.encode() if authenticated else b'<html><body>Login required.</body></html>'
            mime='text/html';status=200 if authenticated else 403
        else:
            if path == '/login': cookie = 'session=fixture; Path=/; Max-Age=3600; HttpOnly; SameSite=Lax';authenticated=True
            storage = '<script>localStorage.setItem("fixture-login","saved");indexedDB.open("fixture-login-db",1);</script>' if path=='/login' else ''
            body = f'<html><head><title>Session fixture</title></head><body><h1>Session fixture</h1><p id="fixture-session">{"Signed in" if authenticated else "Signed out"}</p><a id="fixture-login" href="/login">Login fixture</a>{storage}</body></html>'.encode()
            mime='text/html';status=200
        self.send_response(status)
        if cookie:self.send_header('Set-Cookie',cookie)
        self.send_header('Content-Type',mime);self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
if __name__=='__main__':ThreadingHTTPServer(('127.0.0.1',PORT),Handler).serve_forever()
