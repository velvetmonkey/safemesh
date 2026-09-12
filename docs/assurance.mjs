// Render the two canonical assurance documents through Astro's Markdown pipeline.
import { readFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { unified } from 'unified';
import remarkParse from 'remark-parse';
import remarkGfm from 'remark-gfm';
import remarkSmartypants from 'remark-smartypants';

export const root = fileURLToPath(new URL('../', import.meta.url));
export const pages = { claims: 'CLAIMS.md', proof: 'WHAT-IS-PROVEN.md' };
export const revision = () => execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim();
export const sourceUrl = (path, sha) => `https://github.com/velvetmonkey/safemesh/blob/${sha}/${path}`;
export function walk(node, visit) {
  visit(node);
  for (const child of node.children ?? []) walk(child, visit);
}
export function sourceTree(source, sha) {
  // Match Astro's default GFM syntax and smart punctuation transforms.
  const parser = unified().use(remarkParse).use(remarkGfm).use(remarkSmartypants);
  const tree = parser.runSync(parser.parse(readFileSync(root + source, 'utf8')));
  // Starlight supplies the page's single H1 from its frontmatter.
  if (tree.children[0]?.type !== 'heading' || tree.children[0].depth !== 1) {
    throw new Error(`${source}: expected a source title`);
  }
  tree.children.shift();
  const definitions = new Map();
  walk(tree, node => {
    if (node.type === 'definition' && !definitions.has(node.identifier)) {
      definitions.set(node.identifier, { ...node });
    }
  });
  walk(tree, node => {
    // An image reference can share a definition with a normal link. Resolve its
    // destination separately so the link gets a file view and the image gets bytes.
    if (node.type === 'imageReference' && definitions.has(node.identifier)) {
      const definition = definitions.get(node.identifier);
      node.type = 'image';
      node.url = definition.url;
      node.title = definition.title;
      delete node.identifier;
      delete node.label;
      delete node.referenceType;
    }
    if (!['link', 'definition', 'image'].includes(node.type) || /^(?:[a-z][a-z\d+.-]*:|#|\/)/i.test(node.url)) return;
    const base = node.type === 'image'
      ? `https://raw.githubusercontent.com/velvetmonkey/safemesh/${sha}/${source}`
      : sourceUrl(source, sha);
    node.url = new URL(node.url, base).href;
  });
  return tree;
}
export default function assurance() {
  return (tree, file) => {
    const path = String(file.path).replaceAll('\\', '/');
    const route = Object.keys(pages).find(route => path.endsWith(`/content/docs/${route}.md`));
    if (!route) {
      // Ordinary pages also run this plugin. Only misplaced source markers are errors.
      let marked = false;
      walk(tree, node => { if (node.type === 'html' && node.value.includes('<!-- assurance-source:')) marked = true; });
      if (marked) throw new Error(`${path}: unmatched assurance source page`);
      return;
    }
    const source = pages[route];
    const sha = revision();
    const marker = `<!-- assurance-source: ${source} -->`;
    if (tree.children.length !== 1 || tree.children[0].value !== marker) {
      throw new Error(`${route}: author assurance in ${source}; the page must contain only ${marker}`);
    }
    tree.children = [
      ...unified().use(remarkParse).parse(`Source: [${source}](${sourceUrl(source, sha)}). Build commit: [\`${sha}\`](https://github.com/velvetmonkey/safemesh/tree/${sha}). Main (unreleased); this identifies source, not a successful test run.`).children,
      { type: 'html', value: `<div data-assurance-source="${source}" data-assurance-commit="${sha}">` },
      ...sourceTree(source, sha).children,
      { type: 'html', value: '</div>' },
    ];
  };
}

// Root documents are outside docsLoader's glob. Invalidate these two entries on
// each load, and forward root-file edits to the existing loader during dev.
export function assuranceLoader(loader) {
  return {
    ...loader,
    name: 'assurance-docs-loader',
    async load(context) {
      for (const route of Object.keys(pages)) context.store.delete(route);
      await loader.load(context);
      if (context.watcher) {
        for (const source of Object.values(pages)) context.watcher.add(root + source);
        context.watcher.on('change', changed => {
          for (const [route, source] of Object.entries(pages)) {
            if (changed !== root + source) continue;
            context.store.delete(route);
            context.watcher.emit('change', root + `docs/src/content/docs/${route}.md`);
          }
        });
      }
    },
  };
}
