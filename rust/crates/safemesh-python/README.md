# SafeMesh Python

**v0 scope:** G-Counter and OR-Set are **supported**, within the [language-path limits](https://velvetmonkey.github.io/safemesh/#v0-support). G-Set, PN-Counter, RGA/Text, LWW Register (`LwwRegister`), Enable-wins Flag (`EnableWinsFlag`) and LWW Map (`LwwMap`) are **experimental**, including their deltas and wrappers. Existing proof/test evidence is unchanged by release status.


SafeMesh's Rust crate floor for consumers is **Rust 1.89**. For the source builds,
demos and locked wasm-pack 0.15.0 installation on this page, use **Rust 1.96.1**,
the full-gate CI version. Install rustup first (Linux/Bash, with curl and a native
C compiler/linker), then select that toolchain:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.96.1
. "$HOME/.cargo/env"
rustup default 1.96.1
```

The default applies to your user account; the repository's `rust-toolchain.toml`
also selects 1.96.1 inside this checkout. An outside application's toolchain remains
its own choice; consuming the crate requires at least 1.89.

Install the Python build tool in an activated virtual environment before building:

```sh
python3 -m venv .venv-build
. .venv-build/bin/activate
python3 -m pip install 'maturin>=1.7,<2'
```


`safemesh-python` is the PyO3/maturin wrapper over the SafeMesh Rust core. It exposes byte-oriented replica/event-log helpers for Python code while keeping merge behavior in one Rust implementation.

## Claim boundary

This package provides binding glue with **API present** in `src/lib.rs`.
**Artifact available** means a locally built wheel, not a PyPI release.
**Build checked** and **runtime tested**: CI builds and installs a wheel and runs
the Rust-side binding tests on Linux x64 (`ubuntu-latest`) with CPython 3.8, 3.9,
3.10, 3.11, 3.12, 3.13, and 3.14.
**Integration tested**: each matrix entry runs `examples/data_mule_demo.py` and
`demos/two-app-inventory/test_sync.py` against the installed wheel.
These tests cover modeled in-process convergence and two-app HTTP sync on localhost.
They do not establish other interpreter, Python minor, OS, or architecture coverage.
**Maintainer-supported** status is unknown.
The G-Counter path reaches the Lean-backed Rust carrier; LWW Register,
Enable-wins Flag, and LWW Map remain tested-not-proven.
The binding itself is not a separate proof.

See the repository `CLAIMS.md` and `WHAT-IS-PROVEN.md` for the full wording rule.

## Install

Build a local wheel with maturin; v0.1 CI performs a wheel build and install smoke test but does not publish to PyPI.

The Linux x64 wheel reaches CPython 3.8 and later through PyO3's `abi3-py38` stable ABI. This is
artifact reach, not a claim that every interpreter or platform is tested; macOS, Windows, ARM64,
and other operating-system or architecture combinations have no artifact evidence here.

The Python CI matrix builds and installs the wheel, runs the Rust-side binding tests, and executes
the data-mule demo and two-app inventory sync test on Linux x64 with CPython 3.8, 3.9, 3.10, 3.11,
3.12, 3.13, and 3.14. The `full-gate` job requires every matrix entry to pass and retains its Python
3.11 distribution smoke test. Both use `ubuntu-latest`; other interpreters and future Python minors
are not established by this matrix, and maintainer support remains unknown.

```sh
cd rust/crates/safemesh-python
maturin build --release --features extension-module --out dist
python3 -m venv .venv-smoke
.venv-smoke/bin/pip install --no-index --find-links dist safemesh-python
```

## Quickstart

```python
import safemesh_python as sm

left = sm.GCounterReplica(1, 3)
right = sm.GCounterReplica(2, 3)

assert right.merge_record_bytes(left.append_bump(1, 5)) == "accepted"
admissions = left.merge_log_bytes(right.log_bytes())
assert admissions == ["duplicate"]

print(left.value(), right.value())
```

`merge_record_bytes` returns one `"accepted"`, `"duplicate"`, or `"collision"`
verdict for its record, and `merge_log_bytes` returns one per input record, in
order. Neither raises for a duplicate or a collision, and neither changes state
for one, so check the verdicts for `"collision"`. Decode, ownership, and
whole-batch validation errors raise before any record is applied.

On main (unreleased), `merge_record_bytes` returns this verdict on every replica
class. It previously returned `None` for both accepted and duplicate records and
raised `ValueError: record ID collision` for a collision; code that caught that
exception must check for `"collision"` instead.

## Demo

Narrated walkthrough: [`../../../demos/python-cold-chain/README.md`](../../../demos/python-cold-chain/README.md).

Run the data-mule demo after installing the local wheel:

```sh
cd rust/crates/safemesh-python
.venv-smoke/bin/python examples/data_mule_demo.py
```

It prints `CONVERGED=true` when the sample holder, audit count, and temperature-alert flag converge after partition and heal.

Run `./scripts/package-smoke.sh` from the repository root to build the wheel, install it into a temporary virtualenv, run the demo, and avoid publishing.

## Checked coordinates and OR-Set

`GCounter.try_apply_bump(replica, tally)` raises `IndexError` for an invalid
coordinate without changing state. `apply_bump` uses the same checked path.

```python
left, right = sm.OrSet(), sm.OrSet()
left.add(10, 101)
right.merge(left)
left.apply_remove(left.observed_tokens(10))
right.add(10, 201)  # Concurrent add uses a fresh token.
left.merge(right)
assert left.elements() == [10]
```

The set delegates to Rust `OrSet<u64, u64>`: elements and tokens are unsigned
64-bit integers (JavaScript uses `bigint`). Tokens are global to the set; use a
fresh, replica-unique token for every add to obtain add-wins behavior. Removal
persists tombstones even before an add arrives, and a reused token affects every
element carrying it. Observed tokens include only live adds, excluding tombstoned
tokens. Merge unions all adds and tombstones, and reads return sorted unique live members. This is the
core's token semantics, including token reuse; the binding does not allocate IDs.

## String OR-Set replica with an event log

`StringOrSetReplica` carries a Rust `OrSet<String, u64>` behind an `EventLog`,
so records can be replayed, deduplicated and repaired from a log the same way
`GCounterReplica` does. It is the Python counterpart of the WASM
`SafeMeshStringOrSetReplica`, exposing its replica, read, merge, inspect and
allocated-writer operations in snake_case. The WASM lifecycle methods `free()`
and `[Symbol.dispose]()` have no Python counterpart.
It sits beside `OrSet`, which is unchanged.

```python
left, right = sm.StringOrSetReplica(1), sm.StringOrSetReplica(2)
add = left.append_add("vaccine", 11)
assert right.merge_record_bytes(add) == "accepted"
assert right.merge_record_bytes(add) == "duplicate"  # State unchanged.
right.merge_record_bytes(left.append_remove_observed("vaccine"))
assert right.elements() == [] and right.tombstones() == [11]
assert right.add_entries() == [("vaccine", 11)]
view = sm.StringOrSetReplica.inspect_record_bytes(add)
assert (view.replica(), view.sequence(), view.delta_kind(), view.element(), view.token()) == (
    1, 1, "add", "vaccine", 11)
third = sm.StringOrSetReplica(3)
assert third.merge_log_bytes(left.log_bytes()) == ["accepted", "accepted"]
```

`merge_record_bytes` returns `"accepted"`, `"duplicate"`, or `"collision"`
(the identity is already known with a different payload). Only `"accepted"`
changes state. `merge_log_bytes` returns one of those verdicts per input record;
decode errors raise before any record is applied. Bytes the core cannot decode
raise `failed to decode record: <reason>` or `failed to decode event log:
<reason>`, where the reason is the core's error text. `inspect_record_bytes`
decodes record bytes through the same core decoder without admitting them
anywhere and returns a `StringOrSetRecord` whose `delta_kind()` is `"add"` or
`"remove"`. The three decoding methods accept a keyword-only
`max_collection_elements` budget; exceeding it raises
`maxCollectionElements limit exceeded: <n>`.

Tokens are caller-supplied and global to the set, exactly as for `OrSet` above,
unless the replica is allocated.

### Allocated writers

An allocated replica takes its tokens from the Rust ownership rule instead of
from the caller:

```python
writer = sm.StringOrSetReplica.create_allocated(2, 0)  # writers, author
record = writer.append_allocated_add("water")        # token 2, sequence 1
saved = writer.export_identity()                     # store this yourself
del writer                                           # releases author 0
restored = sm.StringOrSetReplica.import_identity(saved)
restored.append_allocated_add("radio")               # token 4, sequence 2
```

An allocated replica refuses `append_add` with a caller token, and refuses
incoming records whose author, sequence or token break the allocation rule, or
that claim its own author without already being in its log.
`export_identity()` returns local storage bytes (writer count, author, next
sequence, and the full log), not a transport packet. `import_identity` checks
the stored history and never creates a fresh writer when a check fails. It does
not detect a stale snapshot that is consistent with itself, and it does no disk
I/O.

At most one allocated handle per author may be live in one Python process.
A second `create_allocated` or `import_identity` for a live author raises
`author already has a live allocated writer`. The claim is released when the
handle is deallocated. WASM enforces the same rule per WASM instance. Neither
binding fences other processes: two processes that allocate the same author
produce records with the same ID and different payloads, and a reader reports
the second one as `"collision"`. Cross-process exclusion is the caller's job.

### Differences from WASM

- Python uses snake_case method names where WASM uses camelCase.

- Errors raise `ValueError` with the WASM message. WASM's numeric
  `SafeMeshError` code is not carried over. Error texts are the WASM strings
  unchanged, so the append refusal on an allocated replica names the WASM
  method: `allocated replica rejects caller-supplied tokens; use
  appendAllocatedAdd`.
- Integer arguments go through the same checks as the other Python classes: a
  bool raises `TypeError`, a negative or too-large integer raises
  `OverflowError`. WASM raises `SafeMeshError` code 2 with its own text for
  these.
- `max_collection_elements` is keyword-only.
- `add_entries()` returns `(element, token)` tuples where WASM returns entry
  objects with `element()` and `token()`.
- WASM releases an allocated claim on `free()` or `[Symbol.dispose]()`. Python
  has neither lifecycle method; the claim is released when the object is
  deallocated (`del` of the last
  reference, or garbage collection).
- The live-author registry is per Python process instead of per WASM instance,
  because a Python object can be used and dropped on any thread.

`tests/string_orset_wasm_parity.py` runs one list of steps through the
installed wheel and through a Node build of the WASM package, and requires the
same bytes, verdicts, reads and error texts from both, including an
allocated-writer collision. The package smoke script runs it.

## Experimental value classes

`GSet`, `PnCounter`, and `Rga` delegate directly to the Rust value types. Their
`merge(other)` methods mutate the receiver; merging an object with itself is a
no-op. Numeric arguments accept integers, reject booleans, and retain full
unsigned 64-bit precision for elements, positions, values, and tallies.

```python
left, right = sm.GSet(), sm.GSet()
left.insert(10)
right.insert(20)
left.merge(right)
assert left.elements() == [10, 20]

counter = sm.PnCounter(2)
counter.apply_inc(0, 8)
counter.apply_dec(1, 11)
assert counter.value() == -3

sequence = sm.Rga()
sequence.insert(2, 20)
sequence.insert(1, 10)
sequence.delete(2)
assert sequence.live_entries() == [(1, 10)]
```

`GSet.contains(element)` tests membership. `PnCounter` takes the replica count;
`apply_inc`/`apply_dec` and their `try_apply_inc`/`try_apply_dec` aliases take a
replica coordinate and an absolute monotone tally. Invalid coordinates raise
`IndexError`; merging different replica counts raises `ValueError`, without
mutation. `p_state()` and `n_state()` return the component tallies, and `value()`
returns an exact signed Python integer even outside the 64-bit range.

`Rga` uses caller-supplied positions and values, both unsigned 64-bit integers.
`placed()` returns all positioned values, `tombstones()` returns deleted
positions, and `read_positions()` projects the sorted live entries. Distinct
values at the same position remain distinct entries. Deleting a position hides
all its values, including later arrivals. This wrapper does not allocate
positions or implement text editing.

These three classes have no durable `Replica` companion: the Rust core does not
expose public `DurableReplica` construction and restart for these types.
`GSet` and `Rga` expose the Rust core's canonical full-state bytes through
`to_wire_bytes()` and `from_wire_bytes(bytes)`; malformed input raises `ValueError`.
The default decode budget is 4,096 elements per G-Set or RGA collection. An
oversized state raises `ValueError` naming `CollectionElementLimitExceeded` and
the budget. For a trusted larger state, pass an explicit keyword, for example
`GSet.from_wire_bytes(data, max_collection_elements=4097)` or
`Rga.from_wire_bytes(data, max_collection_elements=4097)`. The same canonical
bytes are used at either budget. The keyword accepts `None` (the default) or
a non-bool integer from zero through the platform `usize` maximum. Invalid
values raise a Python type or range error before decoding; zero refuses any
nonempty collection.
Full-state `PnCounter` has no canonical wire form in this version.
