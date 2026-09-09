import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import { siteUrl } from './site-url.mjs';

const site = siteUrl();

export default defineConfig({
  outDir: './dist',
  site: site.origin,
  base: site.pathname.replace(/\/$/, '') || '/',
  integrations: [starlight({
    title: 'SafeMesh documentation',
    components: { Header: './src/components/Header.astro' },
    sidebar: [
      { label: 'Orientation', slug: '' },
      { label: 'Getting started', slug: 'getting-started' },
      { label: 'Concepts', slug: 'concepts' },
      { label: 'Architecture', slug: 'architecture' },
      { label: 'The proof and its boundary', slug: 'proof' },
      { label: 'Using SafeMesh', slug: 'using-safemesh' },
      { label: 'When not to use it', slug: 'limits' },
      { label: 'Claims and evidence', slug: 'claims' },
      { label: 'Examples', slug: 'examples' },
      { label: 'API reference', slug: 'reference' },
    ],
    // Balance guide prose against long, repetitive generated API pages. Keep each
    // heading searchable without a second metadata-title boost overwhelming prose.
    pagefind: {
      ranking: { pageLength: 0.5, metaWeights: { title: 0 } },
    },
  })],
});
