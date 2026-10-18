#!/usr/bin/env python3
"""A test server for the updater: like tools/redirect-server.py (every file behind a redirect, as GitHub does), but the first
N requests for any *.sig answer with garbage, like a cache that still holds the old signature of a new manifest.
  tools/flaky-server.py DIR PORT [N]"""
import http.server, os, socketserver, sys, urllib.parse

root, port = sys.argv[1], int(sys.argv[2])
bad_left = int(sys.argv[3]) if len(sys.argv) > 3 else 2
os.chdir(root)

class H(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        global bad_left
        path = urllib.parse.urlsplit(self.path).path
        if not path.startswith("/r/"):
            self.send_response(302)
            self.send_header("Location", "/r" + self.path)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        if path.endswith(".sig") and bad_left > 0:
            bad_left -= 1
            body = os.urandom(64)
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        self.path = path[2:]
        return super().do_GET()

    def log_message(self, *a):
        pass

socketserver.TCPServer.allow_reuse_address = True
http.server.ThreadingHTTPServer(("", port), H).serve_forever()
