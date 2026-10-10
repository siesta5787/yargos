import http.server, os
class H(http.server.BaseHTTPRequestHandler):
    def do_PUT(self):
        n=int(self.headers.get('Content-Length',0)); os.makedirs('/tmp/aud/up',exist_ok=True)
        open('/tmp/aud/up/'+os.path.basename(self.path),'wb').write(self.rfile.read(n)); self.send_response(200); self.end_headers()
    def log_message(self,*a): pass
http.server.HTTPServer(('0.0.0.0',8002),H).serve_forever()
