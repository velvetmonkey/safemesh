# Native artwork and delivery images

The three PNGs in this directory are the original artwork. Keep them intact.
Generated PNGs live in `delivery/` and are committed so GitHub can render them
directly in the repository README.

Regenerate from the repository root with Node.js 24:

```sh
npm --prefix docs ci
npm --prefix docs run derive:images
```

`docs/derive-images.mjs` uses the locked Sharp dependency, Lanczos3 resizing and
explicit PNG encoding options, without carrying timestamps or source metadata
into the output. Use the same platform and locked dependencies for byte-for-byte
reproduction (the initial output was generated on Linux x64). Running it twice
must produce the same bytes; it never writes an original.

| Original | Current rendered use | Delivery choice |
| --- | --- | --- |
| `safemesh-logo.png` (2172 × 724) | Root README, 300 × 100 CSS pixels | `delivery/safemesh-logo-600.png` (600 × 200), retaining 2× display density |
| `safemesh-hero.png` (1672 × 941) | Root README, responsive width with no explicit size | Keep the original for wide views and high-density displays |
| `safemesh-logo-alt.png` (1254 × 1254) | No current rendered use | Keep the original; no unused derivative |

The documentation site and Lab do not render these images. References in
`docs/assurance.test.mjs` are temporary test probes and deliberately keep the
original paths. The README logo's width, aspect ratio and alt text are unchanged.
To inspect the full-resolution artwork, open the original files above.
