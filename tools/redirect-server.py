#!/usr/bin/env python3
"""Serve a folder the way GitHub serves release files: every request is answered with a redirect, and the file is at
the address it points to. (A plain http.server never redirects, which hid a bug: curl without -L saves the empty redirect
body and reports success.)   tools/redirect-server.py DIR PORT"""
import functools
import http.server
import sys


class Handler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        if self.path.startswith("/r/"):
            self.path = self.path[2:]
            return super().do_GET()
        self.send_response(302)
        self.send_header("Location", "/r" + self.path)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def log_message(self, *args):
        pass


if __name__ == "__main__":
    directory, port = sys.argv[1], int(sys.argv[2])
    http.server.ThreadingHTTPServer(("", port), functools.partial(Handler, directory=directory)).serve_forever()
