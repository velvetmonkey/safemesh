import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
  outDir: './dist',
  integrations: [starlight({
    title: 'SafeMesh documentation',
    components: { Header: './src/components/Header.astro' },
    sidebar: [
      { label: 'Orientation', slug: '' },
      { label: 'Architecture', slug: 'architecture' },
      { label: 'Claims and evidence', slug: 'claims' },
      { label: 'Examples', slug: 'examples' },
    ],
    pagefind: true,
  })],
});
