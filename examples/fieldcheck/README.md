# Fieldcheck · local inspection (Slice 1)

A Linux CLI and a separate Rust process owning one durable UTF-8 OR-Set.
They communicate over stdin/stdout pipes; there are no sockets, peers or
network dependencies at runtime. Building initially may download Cargo dependencies.
Writer configuration is fixed at two writers, this device writer 0. Writer 1
is never created. The checklist is the versioned, compiled-in local asset
`checklist.json`; inspections are immutable JSON string elements.

From the repository root:

```sh
cargo build --locked --manifest-path examples/fieldcheck/Cargo.toml
python3 examples/fieldcheck/fieldcheck.py /absolute/existing/parent/new-store
```

The store directory must initially **not exist**. Its parent must exist.
Every existing store is reopened with checked replay, including empty or damaged
directories; recovery failure preserves the store and disables inspection editing.
Do not delete fence files, copy an active store, or reuse its identity.

At the prompt, after the service PID appears:

```text
draft Fail "Alex" "Latch slips when pulled — check hinge"
save
show
```

The draft gets a UUID and is fsynced to `new-store.draft.json` beside the store
before submission. Keep that file when resolving an uncertain save.
Record field order is fixed; the observed-event-ID array is empty for Slice 1
(and therefore sorted). The returned sequence belongs to SafeMesh, not a clock.

The interface displays `Saving…` until the service responds, then
`Saved on this device` only following successful durable `add`.
A real write error displays `Not saved — local storage error` and retains the
draft. SafeMesh revokes further writes after persistence failure; restart the
service after addressing the storage problem.

## Force-kill journey

In another terminal, run `kill -KILL PID` using the printed **service** PID.
The interface displays `Local service stopped — no inspection data loaded`,
clears loaded records and refuses edits. Enter `reopen`: a new service PID
opens the same store and displays the recovered canonical record, writer and
sequence. Compare these with the pre-kill `show` output. `quit` is a graceful
exit and is not evidence for this journey.

For a lost reply, start with a fresh store and add
`--reply-barrier /absolute/path/reply-pending` to the Python command.
This is an external journey control, not inspection data. After `save`, the
service creates this marker **after successful add and before replying** and
waits while the marker exists. When it appears, SIGKILL the printed service PID.
The interface displays `Save status unknown — reopen to check` followed by the
stopped state. On `reopen`, it finds the retained draft's event ID, checks the
whole record, reports the save and clears the draft without adding another
record. Alternatively, remove the marker to release a live paused response.

For a reproducible real write failure in a disposable store, create a directory
named `writer-0.tmp` inside it after startup but before saving. File creation
then fails with EISDIR; the interface retains the draft. Remove that empty
directory and kill/reopen the service to retry.

`Could not recover this checklist` means replay or application validation
failed. The application never substitutes an empty checklist for a damaged
store. Unsupported schemas, removals, unexpected writers and event-ID
collisions are rejected. Resubmitting exactly the same event returns its original
record and sequence; the same ID with different content is refused.

This adapts the create/restart pattern from `../gold-path/rust`, whose sequential
example has no service/UI scaffold and also demonstrates a second writer.
This slice adds no transport, review policy or browser UI. The forced-kill
journey establishes process-crash recovery on the tested Linux filesystem,
not arbitrary hardware/power-loss guarantees.
