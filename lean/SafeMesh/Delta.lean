/-
SafeMesh — delta-state CRDT convergence, built on crdt-lean.
Copyright (C) 2026 Ben Cassie
SPDX-License-Identifier: AGPL-3.0-or-later
-/
import Crdt

/-!
# Delta-state dissemination for join-semilattice CRDTs

**Motivation (honest).** Full-state gossip re-ships a replica's entire state on
every exchange — too heavy for constrained mesh links (LoRa duty cycles).
Delta-state CRDTs ship only the *change*. Because the crdt-lean carrier is a
join-semilattice (`[SemilatticeSup S] [OrderBot S]`), a delta is just a carrier
element joined in: applying deltas IS the full-state merge, restricted to
smaller payloads. That is why the state-based join variant needs **no
causal-delivery assumption** — order, duplication, and batching of deltas are
all quotiented away by the same associativity/commutativity/idempotence that
powers full-state convergence. The results here are deliberately thin bridges
onto crdt-lean's proven core (`Crdt.strong_eventual_consistency`,
`Crdt.fold_merge_eq_replicaState`, `Crdt.merge_replicaState`); the value is
pinning the bandwidth optimisation to those theorems, not re-proving them.

**Frozen targets (STEP-0, written before proving):**

1. `delta_dissemination_sec` — delta application is order- and
   duplicate-insensitive: two replicas that fold *any* delivery lists with the
   same underlying delta-set hold equal state, and that state is
   `deltaState Δ = Δ.sup id` (`deltaApply_eq_deltaState`).
2. `delta_matches_state` — the bandwidth win costs nothing: if a delta-set `Δ`
   and a full-update-set `U` have equal joins, the delta replica and the
   full-state replica hold identical state. The rfl-grade bridge
   `deltaState_eq_replicaState` is stated explicitly because it IS the point:
   a delta replica *is* a full-state replica over smaller payloads.

Scope guard: the general bridge lives here; the single concrete construction
(delta G-Counter) lives in `SafeMesh.DeltaGCounter`. No delta-intervals, no
anti-entropy protocol layer.
-/

namespace SafeMesh

variable {S : Type*} [SemilatticeSup S] [OrderBot S] [DecidableEq S]

/-- A delta-state replica's state after receiving the finite set `Δ` of deltas:
the join of everything received. Deltas are carrier elements, so this is
definitionally `Crdt.replicaState` — the entire delta optimisation happens on
the wire, not in the convergence argument. -/
def deltaState (Δ : Finset S) : S := Δ.sup id

/-- Incremental delta application: fold the delivery stream into the state with
the CvRDT merge, starting from `⊥`. This is what a mesh node actually executes
as deltas trickle in. -/
def deltaApply (l : List S) : S := l.foldr Crdt.merge ⊥

omit [DecidableEq S] in
/-- The rfl-grade bridge: a delta replica IS a full-state replica. -/
theorem deltaState_eq_replicaState (Δ : Finset S) :
    deltaState Δ = Crdt.replicaState Δ := rfl

/-- Incremental application collapses to the join over the delivered delta-SET:
order and multiplicity of delta delivery are irrelevant. -/
theorem deltaApply_eq_deltaState (l : List S) :
    deltaApply l = deltaState l.toFinset :=
  Crdt.fold_merge_eq_replicaState l

/-- **SEC for delta dissemination.** Two replicas that have folded delivery
lists with the same underlying set of deltas hold equal state — independent of
delivery order and redelivery count. No causal-delivery assumption. -/
theorem delta_dissemination_sec (l₁ l₂ : List S)
    (h : l₁.toFinset = l₂.toFinset) :
    deltaApply l₁ = deltaApply l₂ :=
  Crdt.strong_eventual_consistency l₁ l₂ h

omit [DecidableEq S] in
/-- **The bandwidth win costs nothing.** If a delta decomposition `Δ` and a
full-update-set `U` have the same join, the delta replica and the full-state
replica converge to identical state. -/
theorem delta_matches_state (Δ U : Finset S) (h : Δ.sup id = U.sup id) :
    deltaState Δ = Crdt.replicaState U := h

/-- **Delta gossip step.** Merging two delta replicas equals the delta replica
that has received the union of both delta-sets (inherited from
`Crdt.merge_replicaState`). Pairwise delta gossip therefore converges the mesh
exactly as full-state gossip does. -/
theorem merge_deltaState (Δ₁ Δ₂ : Finset S) :
    Crdt.merge (deltaState Δ₁) (deltaState Δ₂) = deltaState (Δ₁ ∪ Δ₂) :=
  Crdt.merge_replicaState Δ₁ Δ₂

end SafeMesh
