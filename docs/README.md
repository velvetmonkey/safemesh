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
Start it using the existing [Lab instructions](../web/README.md#run); the `dev`, `build` and `preview` commands never build or serve the Lab.
Set `LAB_URL` to an absolute HTTP(S) address before developing or building to link to another existing Lab server, for example `LAB_URL=https://your-existing-lab.example/ npm --prefix docs run build`.

The hosted site is built by `npm --prefix docs run build:site` instead. It runs the same documentation build, then builds the Lab into `docs/dist/lab/` under the site's own base path and passes that address to the documentation build as `LAB_URL`, so the published Lab link is derived from the deployment rather than typed.
`SITE_URL` is the one input: the site's public address including its base path, ending in `/`, defaulting to `https://velvetmonkey.github.io/safemesh/`. `docs/site-url.mjs` resolves it for the Astro config (`site` and `base`), for `build:site` and for the link check, so the same site can be built at another address with, for example, `SITE_URL=https://docs.example.test/preview/ npm --prefix docs run build:site`.
The documentation workflow reads that address from the repository's existing GitHub Pages configuration; it never changes Pages settings.
Building the Lab needs the [Lab's toolchain](../web/README.md#run): Node.js 24, Rust with the `wasm32-unknown-unknown` target, `wasm-pack`, and `npm --prefix web ci`.

`npm --prefix docs ci` installs only the docs dependencies, and `dev`, `build` and `preview` use nothing else, independently of the Rust, WASM and web workflows.
The documentation workflow performs the same clean installs, runs `build:site` and the link check, and deploys the output to GitHub Pages from `main`.

The four content pages are original navigation prose linking to development-branch sources.
They identify `main (unreleased)` in their titles so search results carry the same scope.
Update source documents in their existing locations; do not reproduce their technical prose here.

## Built-output link check

Run `python3 docs/check-links.py` after building. CI runs this same command.
It serves all built HTML (including `404.html`) on an OS-assigned loopback port,
fetches every HTTP(S) `href` and `src`, resolves in-page and cross-page fragments
against real HTML IDs, and fetches external URLs with GET and redirects. GitHub
Markdown fragments accept its `user-content-` ID prefix. Uppercase variants of
internal page paths must return 404, so a case-insensitive server cannot hide typos.
The summary counts link occurrences per class, including repeated references,
separately from unique fetched URLs and case probes. An empty/missing build,
HTTP failure, absent anchor, case-probe failure or crawler exception fails the job.

External checks need the network. Four concurrent workers use ten-second request
timeouts and up to three attempts for transport errors, HTTP 429 and server errors,
with one- and two-second backoffs. Persistent third-party outages still fail CI;
they are reported as external failures and should be retried after recovery, not
silently accepted as healthy links. No external success cache hides link rot.
The elapsed crawl time is printed on every run; CI caps the crawl step at three minutes.

The crawl does not execute JavaScript, inspect CSS URLs or JavaScript imports,
exercise search interactions, validate non-HTTP schemes, or test production-host
routing. Other schemes are counted explicitly as unchecked. The Lab link has no
exemption: it is fetched like every other link, so a build whose Lab link points
at a server that is not running fails the check. Run the check against a
`build:site` output, start the local Lab first, or set `LAB_URL` to a reachable
address. `SITE_URL` sets the default `--site`, so the same command checks a site
built at another address.
