# Documentation development

Use Node.js 22.12 or newer and run these commands from the repository root.

Develop:

```sh
npm --prefix docs ci && npm --prefix docs run dev
```

Build:

```sh
npm --prefix docs ci && npm --prefix docs run build
```

The development server opens at `http://localhost:4321/`; production files and the Pagefind index are written to `docs/dist/`.
To inspect the built search, run `npm --prefix docs run preview` and open `http://localhost:4320/`.
Pagefind is built during the production build, so search verification uses preview.

The global Lab link defaults to `http://localhost:4173/`, the separate Lab's default preview root.
Start it using the existing [Lab instructions](../web/README.md#run); the docs commands never build or serve the Lab.
Set `LAB_URL` to an absolute HTTP(S) address before developing or building to link to another existing Lab server, for example `LAB_URL=https://your-existing-lab.example/ npm --prefix docs run build`.
A future hosted docs build must supply its actual Lab address; the local default is not a deployment configuration.

These commands install only the docs dependencies, independently of the Rust, WASM and web workflows.
The docs CI job performs the same clean install and production build without deploying its output.

The four content pages are original navigation prose linking to development-branch sources.
They identify `main (unreleased)` in their titles so search results carry the same scope.
Update source documents in their existing locations; do not reproduce their technical prose here.
