import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import sharp from 'sharp';

// Keep native artwork intact. Only the README logo has a fixed, smaller display
// size (300 × 100 CSS pixels); retain two source pixels per display pixel.
const assets = new URL('../assets/', import.meta.url);
const output = new URL('delivery/', assets);
const derivative = new URL('safemesh-logo-600.png', output);
const args = process.argv.slice(2);
if (args.length > 1 || (args.length === 1 && args[0] !== '--check')) {
  throw new Error('Usage: node derive-images.mjs [--check]');
}
const expected = await sharp(fileURLToPath(new URL('safemesh-logo.png', assets)))
  .resize({ width: 600, kernel: 'lanczos3', withoutEnlargement: true })
  .png({ compressionLevel: 9, adaptiveFiltering: false, palette: false })
  .toBuffer();

if (args[0] === '--check') {
  let committed;
  try {
    committed = await readFile(derivative);
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
    throw new Error('Missing assets/delivery/safemesh-logo-600.png; run npm --prefix docs run derive:images and commit the result.');
  }
  if (!committed.equals(expected)) {
    throw new Error('Stale assets/delivery/safemesh-logo-600.png; run npm --prefix docs run derive:images and commit the result.');
  }
  console.log('Delivery image matches the current original and derivation settings.');
} else {
  await mkdir(output, { recursive: true });
  await writeFile(derivative, expected);
}
