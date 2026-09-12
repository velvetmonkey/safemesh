#!/usr/bin/env python3
"""Real HTTP regression tests. Optional argument selects another checkout's checker."""
import contextlib
import importlib.util
import io
from functools import partial
from urllib.request import build_opener
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import unittest

CHECKER = Path(sys.argv.pop(1)).resolve() if len(sys.argv) > 1 else Path(__file__).with_name('check-links.py')
spec = importlib.util.spec_from_file_location('checker', CHECKER)
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


class Links(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory(prefix='.link-tests-', dir=CHECKER.parent)
        cls.root = Path(cls.temp.name)
        (cls.root / 'index.html').write_text('<!doctype html>')
        cls.routes = {}

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def do_GET(self):
                status, headers, body = cls.routes.get(self.path, (404, {}, ''))
                self.send_response(status)
                self.send_header('Content-Type', 'text/html')
                for key, value in headers.items():
                    self.send_header(key, value)
                self.end_headers()
                self.wfile.write(body.encode())

        cls.server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()
        cls.base = f'http://127.0.0.1:{cls.server.server_port}/'

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join()
        cls.temp.cleanup()

    def crawl(self, href, body='', routes=None, expected=0, base=None, fetcher=None):
        self.routes.clear()
        self.routes['/'] = (200, {}, f'<a href="{href}">Go</a>{body}')
        self.routes.update(routes or {})
        output = io.StringIO()
        code = 0
        with contextlib.redirect_stdout(output):
            try:
                checker.crawl(self.root, base or self.base, fetcher or checker.fetch)
            except SystemExit as error:
                code = error.code
        self.assertEqual(code, expected, output.getvalue())

    def test_named_anchor(self):
        self.crawl('#section', '<a name="section"></a>')

    def test_top(self):
        for fragment in ('top', 'TOP', 'ToP', '%74op'):
            with self.subTest(fragment=fragment):
                self.crawl('#' + fragment)

    def test_id_literal_decoded_and_named_controls(self):
        for fragment, body in (('section', '<h2 id="section">x</h2>'),
                               ('a%20b', '<h2 id="a%20b">x</h2>'),
                               ('a%20b', '<h2 id="a b">x</h2>'),
                               ('a%20b', '<a name="a%20b"></a>'),
                               ('a%20b', '<a name="a b"></a>')):
            with self.subTest(fragment=fragment, body=body):
                self.crawl('#' + fragment, body)

    def test_missing_fragment(self):
        for fragment, body in (('missing', '<a name="section"></a>'),
                               ('section', '<div name="section"></div>'),
                               ('topper', ''), ('Section', '<a name="section"></a>')):
            with self.subTest(fragment=fragment):
                self.crawl('#' + fragment, body, expected=1)

    def redirect(self, location, body='<h2 id="new-section">x</h2>', href='/old#old-section', expected=0, extra=None, status=302):
        routes = {'/old': (status, {'Location': location}, ''), '/new': (200, {}, body)}
        routes.update(extra or {})
        self.crawl(href, routes=routes, expected=expected)

    def test_redirect_replacement(self):
        for status in (301, 302, 303, 307, 308):
            with self.subTest(status=status):
                self.redirect('new#new-section', status=status)

    def test_redirect_adds_fragment(self):
        self.redirect('/new#new-section', href='/old')
        self.redirect('/new#missing', href='/old', expected=1)

    def test_redirect_inheritance(self):
        self.redirect('/new', body='<h2 id="old-section">x</h2>')
        self.redirect('/new', expected=1)

    def test_redirect_chain(self):
        self.redirect('/middle#new-section', extra={'/middle': (302, {'Location': '/new'}, '')})
        self.redirect('/middle#missing', extra={'/middle': (302, {'Location': '/new'}, '')}, expected=1)

    def test_redirect_empty_fragment(self):
        self.redirect('/middle#', body='', extra={'/middle': (302, {'Location': '/new'}, '')})

    def test_redirect_empty_destination(self):
        self.redirect('/new#', body='<a href="#top">Top</a>')

    def test_redirect_missing_fragment(self):
        self.redirect('/new#missing', expected=1)

    def test_broken_redirect(self):
        self.redirect('/absent#new-section', expected=1)
        self.redirect('/old', expected=1)

    def test_missing_page(self):
        self.crawl('/absent#top', expected=1)

    def test_direct_destination(self):
        self.crawl('/new#new-section', routes={'/new': (200, {}, '<h2 id="new-section">x</h2>')})

    def test_effective_source(self):
        self.routes.clear()
        self.routes.update({
            '/': (302, {'Location': '/dir/page'}, ''),
            '/dir/page': (200, {}, '<a href="target#section">Go</a>'),
            '/dir/target': (200, {}, '<h2 id="section">x</h2>'),
        })
        with contextlib.redirect_stdout(io.StringIO()):
            checker.crawl(self.root, self.base)

    def test_github_effective_origin(self):
        base = 'https://github.com/'
        handlers = [checker.SiteTransport(base, self.server.server_address)]
        if hasattr(checker, 'FragmentRedirectHandler'):
            handlers.append(checker.FragmentRedirectHandler())
        fetcher = partial(checker.fetch, opener=build_opener(*handlers).open, site=base)
        self.crawl('#section', '<h2 id="user-content-section">x</h2>', base=base, fetcher=fetcher)
        self.crawl('/old#section', routes={
            '/old': (302, {'Location': self.base + 'new#section'}, ''),
            '/new': (200, {}, '<h2 id="user-content-section">x</h2>'),
        }, expected=1, base=base, fetcher=fetcher)
        self.crawl('/old#missing', routes={
            '/old': (302, {'Location': base + 'new#section'}, ''),
            '/new': (200, {}, '<h2 id="user-content-section">x</h2>'),
        }, base=base, fetcher=fetcher)

    def test_shared_site_configuration(self):
        # The CLI must fetch its mounted build, even when the declared HTTP
        # origin isn't listening; this mirrors HTTPS production validation.
        (self.root / 'index.html').write_text('<a href="/safemesh/#section">Go</a><h2 id="section">x</h2>')
        values = ('http://127.0.0.1:4321/safemesh/', 'https://example.test/safemesh/',
                  'ftp://example.test/safemesh/', 'http://example.test/a//b/',
                  'http://example.test/safemesh/?query=1', 'http://example.test/safemesh/#section')
        for site in values:
            with self.subTest(site=site):
                builder = subprocess.run(['node', '--input-type=module', '-e',
                    'import {siteUrl} from "./site-url.mjs"; siteUrl(process.argv[1]);', site],
                    cwd=CHECKER.parent, capture_output=True, text=True)
                result = subprocess.run([sys.executable, str(CHECKER), '--root', str(self.root), '--site', site],
                                        capture_output=True, text=True)
                self.assertEqual(result.returncode, 0 if builder.returncode == 0 else 2,
                                 result.stdout + result.stderr)
        (self.root / 'index.html').write_text('<a href="/safemesh/#missing">Go</a>')
        result = subprocess.run([sys.executable, str(CHECKER), '--root', str(self.root), '--site', values[0]],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)


if __name__ == '__main__':
    unittest.main(verbosity=2)
