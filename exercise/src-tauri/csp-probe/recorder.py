"""csp-probe.sh's recorder: prints each beacon path it is sent."""
import http.server, sys
class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        print(self.path, flush=True); self.send_response(204); self.end_headers()
    def log_message(self, *a): pass
http.server.HTTPServer(('127.0.0.1', 7988), H).serve_forever()
