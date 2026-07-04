/-
SafeMesh — delta-state CRDT convergence, built on crdt-lean.
Copyright (C) 2026 Ben Cassie
SPDX-License-Identifier: AGPL-3.0-or-later
-/
import SafeMesh.Delta
import Crdt.ORSet

/-!
# The delta OR-Set

**Frozen targets (STEP-0, written before proving):**

The OR-Set (`Crdt.ORSet.State α τ = Finset (α × τ) × Finset τ`, a product of
two grow-only sets) is the mesh membership/presence workhorse: adds tagged
with unique tokens, removes tombstoning only *observed* tokens, so a
concurrent add (fresh token) beats a remove — add-wins. The carrier is a
proven join-semilattice, so ALL of `SafeMesh.Delta` instantiates at
`S := Crdt.ORSet.State α τ` for free; nothing about convergence is re-proved
here. Deltas, single instance on the wire:

- `orAddDelta a t = ({(a, t)}, ∅)` — one observed add-instance.
- `orRemoveDelta T = (∅, T)` — tombstone the observed token-set `T`.

1. `deltaORSet_adds` / `deltaORSet_tombs` — the accumulation bridges, and
   they collapse *concretely* (Finset-sup of singletons is the set itself):
   a replica that received add-deltas for exactly the instance-set
   `A : Finset (α × τ)` and remove-deltas for the tombstone-sets
   `R : Finset (Finset τ)` holds add-component exactly `A` and tombstone
   component exactly `R.sup id` (the union of shipped tombstone-sets).
2. `deltaORSet_matches_full` (rfl-grade via `deltaState_eq_replicaState`),
   with reads: equal `Crdt.ORSet.elements` and equal `Crdt.ORSet.lookup`
   against a full-state replica fed the same delta-set.
3. `deltaORSet_lookup` — the add-wins read, fully in terms of what was
   shipped: `a` is a member iff SOME shipped add-instance `(a, t) ∈ A` has
   its token `t` outside EVERY shipped tombstone-set (`t ∉ R.sup id`) — via
   `Crdt.ORSet.lookup_iff` + the bridges. A concurrent add's fresh token is
   in no prior remove's tombstone-set, so the add wins; the general
   statement landed, no descope needed.

Scope guard: ONE construction (delta OR-Set), reusing `SafeMesh.Delta`
generically. No Sequence/RGA, no delta-intervals, no anti-entropy layer.
-/

namespace SafeMesh

open Crdt.ORSet (State lookup elements)

variable {α τ : Type*} [DecidableEq α] [DecidableEq τ]

/-- Add-delta: one observed add-instance `(a, t)` on the wire, nothing else. -/
def orAddDelta (a : α) (t : τ) : State α τ := ({(a, t)}, ∅)

/-- Remove-delta: tombstone the observed token-set `T`, nothing else. A remove
computed against local state ships `observedTokens s a` as its `T`. -/
def orRemoveDelta (T : Finset τ) : State α τ := (∅, T)

/-- The delta-set a mesh node accumulates from add-instances `A` and shipped
tombstone-sets `R`. -/
def orDeltas (A : Finset (α × τ)) (R : Finset (Finset τ)) : Finset (State α τ) :=
  (A.image fun p => orAddDelta p.1 p.2) ∪ (R.image orRemoveDelta)

/-- Componentwise sup bridge for an arbitrary product carrier (local helper —
the PN versions are typed at `PNCounter`). -/
private theorem fst_deltaState {A B : Type*} [SemilatticeSup A] [OrderBot A]
    [SemilatticeSup B] [OrderBot B] (Δ : Finset (A × B)) :
    (deltaState Δ).1 = Δ.sup Prod.fst := by
  classical
  induction Δ using Finset.induction_on with
  | empty => simp [deltaState]
  | insert p Δ hp ih =>
    simp only [deltaState, Finset.sup_insert, id_eq] at *
    simp [Prod.fst_sup, ih]

private theorem snd_deltaState {A B : Type*} [SemilatticeSup A] [OrderBot A]
    [SemilatticeSup B] [OrderBot B] (Δ : Finset (A × B)) :
    (deltaState Δ).2 = Δ.sup Prod.snd := by
  classical
  induction Δ using Finset.induction_on with
  | empty => simp [deltaState]
  | insert p Δ hp ih =>
    simp only [deltaState, Finset.sup_insert, id_eq] at *
    simp [Prod.snd_sup, ih]

/-- **The accumulated add-component is exactly the shipped add-instances.**
Finset-sup of singletons is the set itself: receiving `orAddDelta` for each
`(a, t) ∈ A` (in any order, with any redelivery) accumulates precisely `A`;
remove-deltas contribute nothing to the add side. -/
theorem deltaORSet_adds (A : Finset (α × τ)) (R : Finset (Finset τ)) :
    (deltaState (orDeltas A R)).1 = A := by
  classical
  rw [fst_deltaState, orDeltas, Finset.sup_union]
  have hA : (A.image fun p => orAddDelta p.1 p.2).sup Prod.fst = A := by
    rw [Finset.sup_image]
    have : ∀ p : α × τ, (Prod.fst ∘ fun p : α × τ => orAddDelta p.1 p.2) p = {p} := by
      intro p; simp [orAddDelta]
    rw [Finset.sup_congr rfl (fun p _ => this p)]
    induction A using Finset.induction_on with
    | empty => rfl
    | insert p A hp ih => rw [Finset.sup_insert, ih, Finset.sup_eq_union, Finset.insert_eq]
  have hR : (R.image orRemoveDelta).sup Prod.fst = (⊥ : Finset (α × τ)) := by
    rw [Finset.sup_image]
    exact Finset.sup_bot _
  rw [hA, hR, sup_bot_eq]

/-- **The accumulated tombstone-component is the union of shipped
tombstone-sets** (`R.sup id`); add-deltas contribute nothing to it. -/
theorem deltaORSet_tombs (A : Finset (α × τ)) (R : Finset (Finset τ)) :
    (deltaState (orDeltas A R)).2 = R.sup id := by
  classical
  rw [snd_deltaState, orDeltas, Finset.sup_union]
  have hA : (A.image fun p => orAddDelta p.1 p.2).sup Prod.snd = (⊥ : Finset τ) := by
    rw [Finset.sup_image]
    exact Finset.sup_bot _
  have hR : (R.image (orRemoveDelta (α := α))).sup Prod.snd = R.sup id := by
    rw [Finset.sup_image]; rfl
  rw [hA, hR, bot_sup_eq]

/-- A delta OR-Set replica and a full-state OR-Set replica fed the same
delta-set hold identical state — `delta_matches_state` instantiated. -/
theorem deltaORSet_matches_full (Δ : Finset (State α τ)) :
    deltaState Δ = Crdt.replicaState Δ :=
  deltaState_eq_replicaState _

/-- Equal state, equal read: the visible element-sets agree. -/
theorem deltaORSet_elements_match (Δ : Finset (State α τ)) :
    elements (deltaState Δ) = elements (Crdt.replicaState Δ) :=
  congrArg elements (deltaORSet_matches_full Δ)

/-- Equal state, equal membership answers. -/
theorem deltaORSet_lookup_match (Δ : Finset (State α τ)) (a : α) :
    lookup (deltaState Δ) a ↔ lookup (Crdt.replicaState Δ) a := by
  rw [deltaORSet_matches_full]

/-- **The add-wins read, in terms of what was shipped.** After accumulating
add-deltas `A` and remove-deltas `R`, element `a` is a member iff some shipped
add-instance `(a, t)` has its token outside EVERY shipped tombstone-set. A
concurrent add mints a fresh token that no prior remove's `T` contains, so the
add wins — `Crdt.ORSet.lookup_iff` applied through the accumulation bridges. -/
theorem deltaORSet_lookup (A : Finset (α × τ)) (R : Finset (Finset τ)) (a : α) :
    lookup (deltaState (orDeltas A R)) a ↔ ∃ t, (a, t) ∈ A ∧ t ∉ R.sup id := by
  rw [Crdt.ORSet.lookup_iff, deltaORSet_adds, deltaORSet_tombs]

end SafeMesh
