---
title: API reference — main (unreleased)
description: Generated reference for the Rust core API in this source build.
---

This generated reference records **API present** in the source used for this build. It makes no claim about **artifact available**, **build checked**, **runtime tested**, **integration tested**, or **maintainer-supported** for any listed item. Consult the [claims and evidence guide](/safemesh/claims/) for evidence beyond API presence.

## Rust core

[Open the generated `safemesh-crdt` reference](/safemesh/reference/rust/safemesh_crdt/index.html).
The documentation build requires Linux. Cargo runs rustdoc during `npm --prefix docs run build`, using all features on Linux, including `local-writer` and `laws`.
This is a view of main (unreleased), not a version-matched release manual or a portability claim.

Use these entries to move from the guided examples to the API they exercise:

| Task | Rust reference entry | Requirement |
| --- | --- | --- |
| Apply and read G-Counter or OR-Set carrier operations | [`GCounter`](/safemesh/reference/rust/safemesh_crdt/struct.GCounter.html), [`OrSet`](/safemesh/reference/rust/safemesh_crdt/struct.OrSet.html) | No additional feature |
| Admit records and select missing records for repair | [`EventLog::admit_with`](/safemesh/reference/rust/safemesh_crdt/struct.EventLog.html#method.admit_with), [`EventLog::since`](/safemesh/reference/rust/safemesh_crdt/struct.EventLog.html#method.since) | No additional feature |
| Encode and decode records for transport | [`WireEncode::to_wire_bytes`](/safemesh/reference/rust/safemesh_crdt/trait.WireEncode.html#method.to_wire_bytes), [`WireDecode::from_wire_bytes`](/safemesh/reference/rust/safemesh_crdt/trait.WireDecode.html#method.from_wire_bytes) | No additional feature |
| Restart the Rust gold path's durable counter or UTF-8 set | [`DurableReplica::restart_counter`](/safemesh/reference/rust/safemesh_crdt/local/struct.DurableReplica.html#method.restart_counter), [`DurableReplica::restart_utf8_set`](/safemesh/reference/rust/safemesh_crdt/local/struct.DurableReplica.html#method.restart_utf8_set) | Linux, `local-writer` |
| Test a custom type's merge laws and delta convergence | [`check_merge_laws`](/safemesh/reference/rust/safemesh_crdt/laws/fn.check_merge_laws.html), [`check_crdt_convergence`](/safemesh/reference/rust/safemesh_crdt/laws/fn.check_crdt_convergence.html) | `laws`; admission also needs behavioral tests of [`Crdt::validate_record`](/safemesh/reference/rust/safemesh_crdt/trait.Crdt.html#tymethod.validate_record) |

Site search covers all guides and the generated Rust reference, including source pages; it does not index the contents of linked GitHub documents. Rustdoc provides a separate item search for Rust items.
Rustdoc also provides navigation and source links. The “SafeMesh reference guide” link returns here.
External Rust documentation references are displayed as text with their original URL in a tooltip, so browsing and checking this reference does not require a third-party documentation host.
Rustdoc’s “Stable since Rust version” tooltips refer to standard-library API versions; they do not establish SafeMesh support.
