#!/usr/bin/env python3
"""Crawl built HTML over HTTP; any error, including crawler errors, fails CI."""
import argparse
import json
import os
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from functools import partial
from html.parser import HTMLParser
from http.client import HTTPConnection
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from tempfile import TemporaryDirectory
import subprocess
import threading
import time
from urllib.error import HTTPError
from urllib.parse import unquote, urljoin, urlsplit, urlunsplit
from urllib.request import HTTPHandler, HTTPRedirectHandler, HTTPSHandler, Request, build_opener


class Document(HTMLParser):
    def __init__(self, html):
        super().__init__(convert_charrefs=True)
        self.ids, self.names, self.links = set(), set(), []
        self.base = None
        self.feed(html)

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if 'id' in attrs:
            self.ids.add(attrs['id'])
        if tag == 'a' and 'name' in attrs:
            self.names.add(attrs['name'])
        if tag == 'base' and self.base is None and 'href' in attrs:
            self.base = attrs['href']
        for attr in ('href', 'src'):
            if attrs.get(attr) is not None:
                asset = attr == 'src' or tag == 'link' and attrs.get('rel') not in ('canonical', 'alternate')
                self.links.append((attrs[attr], asset, tag == 'base'))

    def has_fragment(self, literal, url):
        # HTML's indicated-part algorithm: literal, then UTF-8 decoded;
        # IDs precede legacy a[name], with top as the final fallback.
        decoded = unquote(literal)
        for fragment in (literal, decoded):
            if fragment in self.ids or fragment in self.names:
                return True
        if urlsplit(url).hostname == 'github.com' and 'user-content-' + decoded in self.ids:
            return True
        return not literal or decoded.lower() == 'top'


class FragmentRedirectHandler(HTTPRedirectHandler):
    """RFC 9110 10.2.2: an absent fragment inherits, an empty one replaces."""

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        location = headers.get('Location', headers.get('URI', ''))
        # urllib resolves Location, but drops an explicit empty fragment and
        # doesn't inherit across changed paths. Retain the delimiter as well.
        if '#' in location:
            suffix = '#' + newurl.split('#', 1)[1] if location.split('#', 1)[1] else '#'
        else:
            suffix = '#' + req.fragment if req.fragment is not None else ''
        newurl = newurl.split('#', 1)[0] + suffix
        return super().redirect_request(req, fp, code, msg, headers, newurl)


    def http_response(self, request, response):
        # Request.full_url/geturl omit a trailing # even though Request.fragment
        # retains the distinction between absent (None) and explicitly empty.
        if request.fragment == '':
            response.url = request.full_url + '#'
        return response

    https_response = http_response


class Handler(SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def list_directory(self, path):
        # Pages does not turn a missing entry page into a directory listing.
        self.send_error(404)
        return None


class SiteTransport(HTTPSHandler, HTTPHandler):
    """Fetch this site's HTTP(S) URLs from the local deployment, without changing paths.

    Other origins retain normal HTTP(S) transport. The request URL and Host header
    stay intact, including on redirects; only this origin's connection is local.
    """

    def __init__(self, site, address):
        super().__init__()
        self.origin = (urlsplit(site).scheme, urlsplit(site).netloc)
        self.address = address

    def http_open(self, request):
        return self.site_open(request, super().http_open)

    def https_open(self, request):
        return self.site_open(request, super().https_open)

    def site_open(self, request, fallback):
        parts = urlsplit(request.full_url)
        if (parts.scheme, parts.netloc) == self.origin:
            return self.do_open(
                lambda host, **kwargs: HTTPConnection(*self.address, **kwargs), request)
        return fallback(request)


def fetch(url, *, opener=None, site=''):
    if opener is None:
        opener = build_opener(FragmentRedirectHandler()).open
    external = (urlsplit(url).hostname != '127.0.0.1' and
                (urlsplit(url).scheme, urlsplit(url).netloc) !=
                (urlsplit(site).scheme, urlsplit(site).netloc))
    for attempt in range(3 if external else 1):
        try:
            with opener(Request(url, headers={'User-Agent': 'SafeMesh-docs-link-check/1.0'}), timeout=10) as response:
                return response.status, response.read().decode('utf-8', errors='replace'), response.headers.get('Content-Type', ''), response.geturl()
        except HTTPError as error:
            if error.code < 500 and error.code != 429:
                return error.code, '', '', error.geturl()
            failure = str(error)
        except OSError as error:
            failure = str(error)
        if external and attempt < 2:
            time.sleep(attempt + 1)
    raise RuntimeError(f'{url}: {failure}')


def resolve_documents(documents):
    """Resolve href/src and the first base with the browser's WHATWG URL rules.

    Node is already required to build this site. Batch the complete crawl through
    its URL implementation instead of maintaining a second URL parser in Python.
    Invalid URLs remain errors; an invalid base falls back to the document URL
    for other links, as in the browser, but is itself still checked.
    """
    result = subprocess.run(['node', '-e', r"""
const fs = require('node:fs');
const resolve = (raw, base) => {
    try { return new URL(raw, base).href; }
    catch { return null; }
};
const documents = JSON.parse(fs.readFileSync(0, 'utf8'));
process.stdout.write(JSON.stringify(documents.map(({source, base, links}) => {
    let documentBase = base === null ? source : resolve(base, source) ?? source;
    // HTML's frozen-base rule uses the document URL for these schemes.
    // This changes resolution only: every base/link is still returned below.
    const protocol = new URL(documentBase).protocol;
    if (protocol === 'data:' || protocol === 'javascript:') documentBase = source;
    return links.map(([raw, asset, isBase]) =>
        resolve(raw, isBase ? source : documentBase));
})));
"""], input=json.dumps([
        dict(source=source, base=document.base, links=document.links)
        for _, source, document in documents
    ]), text=True, capture_output=True, check=True)
    return json.loads(result.stdout)


def crawl(root, base, fetcher=fetch):
    pages = sorted(root.rglob('*.html'))
    if not pages or not (root / 'index.html').is_file():
        raise RuntimeError(f'No built entry page in {root}')
    counts, errors, references, probes = Counter(), [], [], set()
    parsed = []
    for page in pages:
        route = urlsplit(base).path + page.relative_to(root).as_posix()
        if route.endswith('index.html'):
            route = route[:-10]
        source = urljoin(base, route)
        status, body, _, effective = fetcher(source)
        if status != 200:
            errors.append(f'{route}: HTTP {status}')
        parsed.append((route, effective, Document(body)))
    for (route, source, document), targets in zip(parsed, resolve_documents(parsed), strict=True):
        for (raw, asset, is_base), target in zip(document.links, targets, strict=True):
            if target is None:
                errors.append(f'{route} -> {raw!r}: invalid browser URL')
                continue
            parts = urlsplit(target)
            reference = urlsplit(raw.strip())
            # Explicit page names may deliberately link to the current page (for
            # example rustdoc type names). Empty destinations and dot-only paths
            # supply no page name; reject those when they cannot navigate anywhere.
            unnamed = (not reference.scheme and not reference.netloc and
                       not reference.path.startswith('/') and
                       all(part in ('', '.', '..') for part in reference.path.split('/')))
            # A base element configures resolution; it is not a visitor link.
            # Its HTTP destination is still checked below like every other href.
            if not is_base and unnamed and not reference.fragment and not reference.query and (
                    not reference.path or target == source):
                errors.append(f'{route} -> {raw!r}: empty or degenerate target')
            if parts.scheme not in ('http', 'https'):
                counts['OTHER-SCHEME (unchecked)'] += 1
                continue
            local = (parts.scheme, parts.netloc) == (urlsplit(base).scheme, urlsplit(base).netloc)
            fragment = unquote(parts.fragment)
            # Preserve even an empty query delimiter in the browser's URL.
            url = target.split('#', 1)[0]
            kind = ('EXTERNAL' if not local else 'INTERNAL-ASSET' if asset else
                    'ANCHOR-SAME-PAGE' if fragment and parts.path == urlsplit(source).path else
                    'ANCHOR-CROSS-PAGE' if fragment else 'INTERNAL-PAGE')
            counts[kind] += 1
            references.append((route, raw, url, fragment, kind, parts.fragment))
            if local and kind != 'INTERNAL-ASSET' and parts.path.upper() != parts.path:
                probes.add(urlunsplit(parts._replace(path=parts.path.upper(), fragment='')))
                # Also probe case within the mount: uppercasing only the mount
                # would miss case-insensitive routes beneath a case-sensitive base.
                prefix = urlsplit(base).path
                if parts.path.startswith(prefix):
                    case_path = prefix + parts.path[len(prefix):].upper()
                    if case_path != parts.path:
                        probes.add(urlunsplit(parts._replace(path=case_path, fragment='')))
    urls = sorted({r[2] for r in references} | probes)
    with ThreadPoolExecutor(max_workers=4) as pool:
        results = dict(zip(urls, pool.map(fetcher, urls)))
    # Parse each fetched document once, even when rustdoc links to many anchors.
    documents = {}
    for route, raw, url, fragment, kind, literal_fragment in references:
        status, body, content_type, effective = results[url]
        # Fetch each fragmentless URL once. If no redirect supplied a fragment,
        # the original reference inherits unchanged; otherwise use the last
        # replacement (including an explicit empty fragment).
        if '#' in effective:
            literal_fragment = effective.split('#', 1)[1]
            fragment = unquote(literal_fragment)
        problem = None
        if status != 200:
            problem = f'HTTP {status}'
        elif fragment and kind != 'INTERNAL-ASSET':
            if url not in documents:
                documents[url] = Document(body)
            if not documents[url].has_fragment(literal_fragment, effective):
                problem = f'missing id #{fragment}'
        if problem:
            errors.append(f'{route} -> {raw} [{kind}] {problem}')
    for url in sorted(probes):
        if results[url][0] != 404:
            errors.append(f'CASE-PROBE {url}: expected 404, got {results[url][0]}')
    for kind in ('INTERNAL-PAGE', 'INTERNAL-ASSET', 'ANCHOR-SAME-PAGE', 'ANCHOR-CROSS-PAGE', 'EXTERNAL', 'OTHER-SCHEME (unchecked)'):
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
    parser.add_argument('--site', default=os.environ.get('SITE_URL') or 'https://velvetmonkey.github.io/safemesh/',
                        help='Deployed HTTP(S) site URL, including its base path (default: SITE_URL, else the production site)')
    args = parser.parse_args()
    # Use the builder's parser, including WHATWG normalization and validation,
    # rather than allowing the two SITE_URL contracts to drift.
    resolved = subprocess.run(['node', '--input-type=module', '-e',
        'import {siteUrl} from "./site-url.mjs"; process.stdout.write(siteUrl(process.argv[1]).href);',
        args.site], cwd=Path(__file__).parent, text=True, capture_output=True)
    if resolved.returncode:
        parser.error('--site: ' + resolved.stderr.strip())
    args.site = resolved.stdout
    site = urlsplit(args.site)
    root = args.root.resolve(strict=True)
    start = time.monotonic()
    # A real directory mount preserves /safemesh/ in every HTTP request.
    # Keep temporary files beside the build, never in a system temporary directory.
    with TemporaryDirectory(prefix='.crawl-', dir=root.parent) as staging:
        document_root = Path(staging)
        if site.path == '/':
            document_root = root
        else:
            mount = document_root / site.path.lstrip('/')
            mount.parent.mkdir(parents=True, exist_ok=True)
            mount.symlink_to(root, target_is_directory=True)
        server = ThreadingHTTPServer(('127.0.0.1', 0),
                                     partial(Handler, directory=str(document_root)))
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        opener = build_opener(SiteTransport(args.site, server.server_address), FragmentRedirectHandler())
        fetcher = partial(fetch, opener=opener.open, site=args.site)
        try:
            print(f'Serving {root} at {args.site} via 127.0.0.1:{server.server_port}')
            crawl(root, args.site, fetcher)
        finally:
            server.shutdown()
            server.server_close()
            thread.join()
            print(f'CRAWL SECONDS {time.monotonic() - start:.2f}')


if __name__ == '__main__':
    main()
