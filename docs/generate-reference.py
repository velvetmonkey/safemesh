#!/usr/bin/env python3
"""Build the Rust surface with rustdoc into the already-built documentation site."""
import os
import re
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from html import escape
from html.parser import HTMLParser
import xml.etree.ElementTree as ET
from urllib.parse import urlsplit


class LocalReference(HTMLParser):
    """Keep rustdoc markup, presenting external references as labelled text.

    Local links, including source and anchor links, remain links for the site's
    crawler. External URLs remain inspectable in title attributes.
    """

    def __init__(self, header):
        super().__init__(convert_charrefs=False)
        self.parts = []
        self.anchors = []
        self.root_page = False
        self.header = header

    def handle_starttag(self, tag, attrs):
        attributes = dict(attrs)
        classes = set(attributes.get('class', '').split())
        if tag == 'main':
            self.parts.extend([self.get_starttag_text(), self.header])
            return
        # Rustdoc's content section is the search body, not its surrounding UI.
        if tag == 'section' and attributes.get('id') == 'main-content':
            attrs.append(('data-pagefind-body', None))
        # API signatures and implementation headings repeat identifiers heavily.
        # Retain the content, with lower weight than authored guide prose.
        if (tag in {'section', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'pre', 'p', 'li'}
                or 'docblock' in classes):
            attrs.append(('data-pagefind-weight', '0.1'))
        # Exclude controls by element/role and rustdoc's structural chrome classes.
        # Keep ordinary summary headings and field-name anchors: those are API text.
        if (tag in {'button', 'input', 'select', 'textarea', 'form', 'nav'}
                or attributes.get('role') == 'button'
                or classes & {'rustdoc-breadcrumbs', 'out-of-band'}
                or ('src' in classes and (tag == 'a' or 'rightside' in classes))
                or (tag == 'a' and 'anchor' in classes and 'field' not in classes)
                or (tag == 'summary' and 'hideme' in classes)):
            attrs.append(('data-pagefind-ignore', 'all'))
        start = '<' + tag + ''.join(
            f' {k}' if v is None else f' {k}="{escape(v, quote=True)}"'
            for k, v in attrs) + '>'
        if tag == 'a':
            url = dict(attrs).get('href', '')
            external = bool(urlsplit(url).netloc)
            self.anchors.append(external)
            # Give rustdoc's page-heading links a real destination. Tooltip
            # handlers retain their classes and JS behavior, with the same useful
            # content destination as a fallback when JavaScript is unavailable.
            if url == '#':
                url = '#main-content'
            if not external and ('/src/' in url or url.startswith('src/')):
                # Rustdoc highlights line ranges with JS; link to the actual first
                # line ID so source navigation also works without JavaScript.
                url = re.sub(r'#(\d+)-\d+$', r'#\1', url)
            if self.root_page and url == './index.html':
                url = './safemesh_crdt/index.html'
            if not external and url != dict(attrs).get('href', ''):
                self.parts.append('<a' + ''.join(
                    f' {k}' if v is None else f' {k}="{escape(url if k == "href" else v, quote=True)}"'
                    for k, v in attrs) + '>')
                return
            if external:
                attrs = [(k, v) for k, v in attrs if k not in ('href', 'title', 'target', 'rel')]
                attrs.append(('title', f'External reference: {url}'))
                self.parts.append('<span' + ''.join(
                    f' {k}' if v is None else f' {k}="{escape(v, quote=True)}"'
                    for k, v in attrs) + '>')
                return
        self.parts.append(start)

    def handle_endtag(self, tag):
        if tag == 'a' and self.anchors.pop():
            tag = 'span'
        self.parts.append(f'</{tag}>')

    def handle_startendtag(self, tag, attrs):
        self.parts.append(self.get_starttag_text())

    def handle_data(self, data):
        self.parts.append(data)

    def handle_entityref(self, name):
        self.parts.append(f'&{name};')

    def handle_charref(self, name):
        self.parts.append(f'&#{name};')

    def handle_comment(self, data):
        self.parts.append(f'<!--{data}-->')

    def handle_decl(self, decl):
        self.parts.append(f'<!{decl}>')


def main():
    if sys.platform != "linux":
        raise SystemExit("Build the Rust reference on Linux to include local-writer APIs.")
    docs = Path(__file__).resolve().parent
    target = Path(os.environ.get('CARGO_TARGET_DIR', docs / '.reference-target')).resolve()
    target.mkdir(parents=True, exist_ok=True)
    destination = docs / 'dist/reference/rust'
    # A fresh target prevents removed or renamed items surviving incremental docs.
    # Each future surface can generate into its own dist/reference/<surface> path.
    with tempfile.TemporaryDirectory(prefix='rustdoc-', dir=target) as build:
        env = dict(os.environ, CARGO_TARGET_DIR=build)
        subprocess.run([
            'cargo', 'doc', '--manifest-path', str(docs.parent / 'rust/Cargo.toml'),
            '-p', 'safemesh-crdt', '--all-features', '--no-deps',
        ], env=env, check=True)
        if destination.exists():
            shutil.rmtree(destination)
        shutil.copytree(Path(build) / 'doc', destination)
    for page in destination.rglob('*.html'):
        rendered = LocalReference((docs / 'reference-header.html').read_text())
        rendered.root_page = page.parent == destination
        rendered.feed(page.read_text())
        rendered.close()
        page.write_text(''.join(rendered.parts))
    # The reference is public, navigable documentation, including linked source.
    # Keep the linked rustdoc Help and Settings routes discoverable too; their
    # explanatory text is public, while interactive controls are not search content.
    # Include it in the sitemap after generation; Astro only knows authored routes.
    namespace = 'http://www.sitemaps.org/schemas/sitemap/0.9'
    ET.register_namespace('', namespace)
    sitemap = docs / 'dist/sitemap-0.xml'
    tree = ET.parse(sitemap)
    root = tree.getroot()
    site = root.find(f'{{{namespace}}}url/{{{namespace}}}loc').text
    # Re-running this generator replaces reference entries, including removed items.
    for entry in list(root):
        location = entry.find(f'{{{namespace}}}loc')
        if location is not None and location.text.startswith(site + 'reference/rust/'):
            root.remove(entry)
    for page in sorted(destination.rglob('*.html')):
        route = page.relative_to(docs / 'dist').as_posix()
        if route.endswith('index.html'):
            route = route[:-10]
        entry = ET.SubElement(root, f'{{{namespace}}}url')
        ET.SubElement(entry, f'{{{namespace}}}loc').text = site + route
    tree.write(sitemap, encoding='utf-8', xml_declaration=True)
    print(f'Generated Rust reference: {len(list(destination.rglob("*.html")))} HTML pages')


if __name__ == '__main__':
    main()
