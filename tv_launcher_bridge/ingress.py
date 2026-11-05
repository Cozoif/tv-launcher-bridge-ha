"""HA Ingress pairing UI and lifecycle supervisor; no third-party Python packages."""
import json
import os
from pathlib import Path
import signal
import subprocess
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.request import Request, urlopen
from urllib.error import HTTPError

DATA = Path('/data/tv-launcher-bridge')
ALLOWED = {'/manage/state', '/manage/discover', '/manage/pair', '/manage/configure',
           '/manage/connect', '/manage/apps', '/status'}


def prepare(options, data=DATA):
    token = options.get('api_token', '').strip()
    if len(token) < 24:
        raise ValueError('Set api_token to a unique secret of at least 24 characters in add-on Configuration')
    data.mkdir(parents=True, exist_ok=True, mode=0o700)
    path = data / 'settings.json'
    config = json.loads(path.read_text()) if path.exists() else {'tv_port': 0}
    config.update({key: options[key] for key in
                   ('jellyfin_url', 'jellyfin_api_key', 'jellyfin_device', 'allow_raw_commands')
                   if key in options})
    config.update(api_token=token, api_port=8125, auto_find_port=True)
    temp = path.with_suffix('.tmp')
    temp.write_text(json.dumps(config))
    temp.chmod(0o600)
    temp.replace(path)
    return token


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass  # Never record pairing codes, tokens or request bodies.

    def respond(self, status, body, content='application/json'):
        self.send_response(status)
        self.send_header('Content-Type', content)
        self.send_header('Cache-Control', 'no-store')
        self.send_header('X-Content-Type-Options', 'nosniff')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        self.handle_request()

    def do_POST(self):
        self.handle_request()

    def handle_request(self):
        # Trust the socket peer only, never forwarded headers supplied by clients.
        if self.client_address[0] != '172.30.32.2':
            return self.respond(403, b'{"ok":false,"message":"Use Home Assistant Ingress"}')
        path = self.path.split('?')[0]
        if self.command == 'GET' and path in ('', '/'):
            return self.respond(200, Path(__file__).with_name('index.html').read_bytes(), 'text/html; charset=utf-8')
        if path not in ALLOWED or (self.command == 'GET' and path not in ('/manage/state', '/status')):
            return self.respond(404, b'{"ok":false}')
        if self.command == 'POST' and self.headers.get('X-TV-Bridge') != 'ingress':
            return self.respond(403, b'{"ok":false,"message":"Missing UI request header"}')
        try:
            size = int(self.headers.get('Content-Length', 0))
            if size < 0 or size > 8192:
                return self.respond(413, b'{"ok":false}')
            body = self.rfile.read(size) if self.command == 'POST' else None
            request = Request(self.server.api_base + path, body, method=self.command,
                              headers={'Authorization': 'Bearer ' + self.server.token,
                                       'Content-Type': 'application/json'})
            try:
                with urlopen(request, timeout=120) as response:
                    self.respond(response.status, response.read())
            except HTTPError as error:
                self.respond(error.code, error.read())
        except Exception:
            self.respond(502, b'{"ok":false,"message":"Bridge unavailable or request timed out; inspect add-on logs"}')


def main():
    os.umask(0o077)
    options = json.loads(Path('/data/options.json').read_text())
    token = prepare(options)
    bind = options.get('api_bind', '127.0.0.1')
    env = dict(os.environ, XDG_CONFIG_HOME='/data', TV_BRIDGE_BIND=bind)
    child = subprocess.Popen(['/usr/local/bin/tv-bridge-addon'], env=env)
    server = ThreadingHTTPServer(('0.0.0.0', 8099), Handler)
    server.token = token
    # Wildcard binding is optional for the TV companion on a trusted LAN.
    server.api_base = 'http://' + ('127.0.0.1' if bind == '0.0.0.0' else bind) + ':8125'
    def stop(*_):
        child.terminate()
    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        raise SystemExit(child.wait())
    finally:
        server.shutdown()
        server.server_close()


if __name__ == '__main__':
    main()
