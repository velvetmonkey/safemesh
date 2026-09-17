import { mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import sharp from 'sharp';

// Keep native artwork intact. Only the README logo has a fixed, smaller display
// size (300 × 100 CSS pixels); retain two source pixels per display pixel.
const assets = new URL('../assets/', import.meta.url);
const output = new URL('delivery/', assets);
await mkdir(output, { recursive: true });
await sharp(fileURLToPath(new URL('safemesh-logo.png', assets)))
  .resize({ width: 600, kernel: 'lanczos3', withoutEnlargement: true })
  .png({ compressionLevel: 9, adaptiveFiltering: false, palette: false })
  .toFile(fileURLToPath(new URL('safemesh-logo-600.png', output)));
