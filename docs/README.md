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
routing; it also does not fetch the exact default `http://localhost:4173/` link to
the separately started local Lab. Those Lab links and other schemes are counted explicitly as unchecked.
A configured public Lab URL is fetched like every other external URL.
