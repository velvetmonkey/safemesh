#!/usr/bin/env python3
"""Crawl built HTML over HTTP; any error, including crawler errors, fails CI."""
import argparse
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from functools import partial
from html.parser import HTMLParser
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import threading
import time
from urllib.error import HTTPError
from urllib.parse import unquote, urljoin, urlsplit, urlunsplit
from urllib.request import Request, urlopen


class Document(HTMLParser):
    def __init__(self, html):
        super().__init__(convert_charrefs=True)
        self.ids, self.links = set(), []
        self.feed(html)

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if 'id' in attrs:
            self.ids.add(attrs['id'])
        for attr in ('href', 'src'):
            if attrs.get(attr) is not None:
                asset = attr == 'src' or tag == 'link' and attrs.get('rel') not in ('canonical', 'alternate')
                self.links.append((attrs[attr], asset))


class Handler(SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass


def fetch(url):
    external = urlsplit(url).hostname != '127.0.0.1'
    for attempt in range(3 if external else 1):
        try:
            with urlopen(Request(url, headers={'User-Agent': 'SafeMesh-docs-link-check/1.0'}), timeout=10) as response:
                return response.status, response.read().decode('utf-8', errors='replace'), response.headers.get('Content-Type', '')
        except HTTPError as error:
            if error.code < 500 and error.code != 429:
                return error.code, '', ''
            failure = str(error)
        except OSError as error:
            failure = str(error)
        if external and attempt < 2:
            time.sleep(attempt + 1)
    raise RuntimeError(f'{url}: {failure}')


def crawl(root, base):
    pages = sorted(root.rglob('*.html'))
    if not pages or not (root / 'index.html').is_file():
        raise RuntimeError(f'No built entry page in {root}')
    counts, errors, references, probes = Counter(), [], [], set()
    for page in pages:
        route = '/' + page.relative_to(root).as_posix()
        if route.endswith('index.html'):
            route = route[:-10]
        source = urljoin(base, route)
        status, body, _ = fetch(source)
        if status != 200:
            errors.append(f'{route}: HTTP {status}')
        for raw, asset in Document(body).links:
            target = urljoin(source, raw)
            parts = urlsplit(target)
            if parts.scheme not in ('http', 'https'):
                counts['OTHER-SCHEME (unchecked)'] += 1
                continue
            local = parts.netloc == urlsplit(base).netloc
            if not local and target == 'http://localhost:4173/':
                # The documented separate local Lab is not a docs build artifact.
                counts['EXTERNAL-LOCAL-LAB (unchecked)'] += 1
                continue
            fragment = unquote(parts.fragment)
            url = urlunsplit(parts._replace(fragment=''))
            kind = ('EXTERNAL' if not local else 'INTERNAL-ASSET' if asset else
                    'ANCHOR-SAME-PAGE' if fragment and parts.path == urlsplit(source).path else
                    'ANCHOR-CROSS-PAGE' if fragment else 'INTERNAL-PAGE')
            counts[kind] += 1
            references.append((route, raw, url, fragment, kind))
            if local and kind != 'INTERNAL-ASSET' and parts.path.upper() != parts.path:
                probes.add(urlunsplit(parts._replace(path=parts.path.upper(), fragment='')))
    urls = sorted({r[2] for r in references} | probes)
    with ThreadPoolExecutor(max_workers=4) as pool:
        results = dict(zip(urls, pool.map(fetch, urls)))
    for route, raw, url, fragment, kind in references:
        status, body, content_type = results[url]
        problem = None
        if status != 200:
            problem = f'HTTP {status}'
        elif fragment and kind != 'INTERNAL-ASSET':
            ids = Document(body).ids
            # GitHub namespaces rendered Markdown headings with user-content-.
            if fragment not in ids and not (urlsplit(url).hostname == 'github.com' and 'user-content-' + fragment in ids):
                problem = f'missing id #{fragment}'
        if problem:
            errors.append(f'{route} -> {raw} [{kind}] {problem}')
    for url in sorted(probes):
        if results[url][0] != 404:
            errors.append(f'CASE-PROBE {url}: expected 404, got {results[url][0]}')
    for kind in ('INTERNAL-PAGE', 'INTERNAL-ASSET', 'ANCHOR-SAME-PAGE', 'ANCHOR-CROSS-PAGE', 'EXTERNAL', 'EXTERNAL-LOCAL-LAB (unchecked)', 'OTHER-SCHEME (unchecked)'):
        print(f'{kind}: {counts[kind]}')
    print(f'ROUTES {len(pages)}, LINKS CHECKED {len(references)}, UNIQUE FETCHES {len(urls)}, CASE PROBES {len(probes)}, BROKEN {len(errors)}')
    for error in errors:
        print('RED:', error)
    if errors:
        raise SystemExit(1)
    print('GREEN: all checked links resolve')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).parent / 'dist')
    args = parser.parse_args()
    root = args.root.resolve(strict=True)
    start = time.monotonic()
    server = ThreadingHTTPServer(('127.0.0.1', 0), partial(Handler, directory=str(root)))
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        print(f'Serving {root} on 127.0.0.1:{server.server_port}')
        crawl(root, f'http://127.0.0.1:{server.server_port}/')
    finally:
        server.shutdown()
        server.server_close()
        thread.join()
        print(f'CRAWL SECONDS {time.monotonic() - start:.2f}')


if __name__ == '__main__':
    main()
