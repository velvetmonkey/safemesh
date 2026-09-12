// Compare the actual built article to a fresh, independent read of its source.
// No stored digest or generated Markdown can make stale output pass this check.
import { readFileSync } from 'node:fs';
import { unified } from 'unified';
import remarkRehype from 'remark-rehype';
import rehypeRaw from 'rehype-raw';
import { parse } from 'parse5';
import { pages, root, revision, sourceTree, walk } from './assurance.mjs';

const blocks = new Set(['p', 'div', 'li', 'ul', 'ol', 'blockquote', 'h1', 'h2', 'h3', 'h4', 'pre', 'br']);
function signature(node) {
  let text = '';
  const links = [];
  const code = [];
  const images = [];
  function codeText(n) {
    if (n.type === 'text' || n.nodeName === '#text') return n.value;
    const attrs = n.properties ?? Object.fromEntries((n.attrs ?? []).map(a => [a.name, a.value]));
    if (n.tagName === 'a') links.push(attrs.href);
    if (n.tagName === 'img') images.push({ src: attrs.src, alt: attrs.alt ?? '' });
    const classes = String(attrs.className ?? attrs.class ?? '').split(/[ ,]+/);
    const children = n.children ?? n.childNodes ?? [];
    // Expressive Code represents newlines as ec-line containers.
    if (classes.includes('ec-line')) {
      const content = children.find(c => (c.attrs ?? []).some(a => a.name === 'class' && a.value.split(' ').includes('code')));
      return codeText(content ?? { children }) + '\n';
    }
    return children.map(codeText).join('');
  }
  function visit(n) {
    const attrs = n.properties ?? Object.fromEntries((n.attrs ?? []).map(a => [a.name, a.value]));
    const tag = n.tagName;
    // Starlight adds heading-permalink controls, outside authored content.
    if (String(attrs.className ?? attrs.class ?? '').includes('sl-anchor-link')) return;
    if (tag === 'pre' || tag === 'code') {
      // Highlighting adds spans, but indentation and line breaks remain content.
      const value = codeText(n);
      code.push({ tag, value });
      text += ` code:${code.length} `;
      return;
    }
    if (n.type === 'text' || n.nodeName === '#text') text += n.value;
    if (tag === 'a') links.push(attrs.href);
    if (tag === 'img') images.push({ src: attrs.src, alt: attrs.alt ?? '' });
    for (const c of n.children ?? n.childNodes ?? []) visit(c);
    if (blocks.has(tag)) text += ' ';
  }
  visit(node);
  return { text: text.replace(/\s+/g, ' ').trim(), links, code, images };
}
let total = 0;
for (const [route, source] of Object.entries(pages)) {
  const html = parse(readFileSync(root + `docs/dist/${route}/index.html`, 'utf8'));
  const matches = [];
  function find(n) {
    if (n.attrs?.some(a => a.name === 'data-assurance-source' && a.value === source)) matches.push(n);
    for (const c of n.childNodes ?? []) find(c);
  }
  find(html);
  if (matches.length !== 1) throw new Error(`${route}: expected exactly one source article`);
  const sha = revision();
  if (!matches[0].attrs.some(a => a.name === 'data-assurance-commit' && a.value === sha)) {
    throw new Error(`${route}: stale build commit`);
  }
  const tree = sourceTree(source, sha);
  const expected = signature(await unified().use(remarkRehype, { allowDangerousHtml: true }).use(rehypeRaw).run(tree));
  const actual = signature(matches[0]);
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`${route}: rendered text or evidence links disagree with ${source}`);
  }
  if (route === 'claims') {
    // Every listed statement (including the parent kernel scope) plus the
    // opening conditional claim counts once. Nested claims count separately.
    let count = 0, missing = 0, proven = 0;
    walk(tree, node => {
      if (!['listItem', 'blockquote'].includes(node.type)) return;
      const own = { type: 'root', children: node.children.filter(c => c.type !== 'list') };
      let kinds = [], evidence = 0;
      walk(own, n => {
        if (n.type === 'strong') {
          const value = n.children.map(c => c.value ?? '').join('');
          if (['PROVEN', 'TESTED', 'NOT COVERED'].includes(value)) kinds.push(value);
        }
        if (n.type === 'link' && n.url.startsWith('https://github.com/velvetmonkey/safemesh/blob/')) evidence++;
      });
      count++;
      if (!kinds.length || !evidence) missing++;
      if (kinds.includes('PROVEN')) proven++;
    });
    if (!count || missing) throw new Error(`claims: ${count} claims, ${missing} without kind/evidence`);
    console.log(`CLAIMS ${count}, WITHOUT EVIDENCE ${missing}, PROVEN ${proven}`);
  }
  total++;
  console.log(`GREEN: ${route} text and ${actual.links.length} evidence links match ${source}`);
}
console.log(`ASSURANCE PAGES ${total}, DRIFT 0`);
