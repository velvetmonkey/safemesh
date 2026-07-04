/-
SafeMesh — delta-state CRDT convergence, built on crdt-lean.
Copyright (C) 2026 Ben Cassie
SPDX-License-Identifier: AGPL-3.0-or-later
-/
import SafeMesh.Delta
import Crdt.Sequence

/-!
# The delta RGA (replicated ordered sequence)

**Frozen targets (STEP-0, written before proving):**

The RGA-family sequence (`Crdt.RGA.State ι α = Finset (ι × α) × Finset ι`:
positioned elements + tombstoned positions, `[LinearOrder ι]`) is the same
product-of-G-Sets shape as the OR-Set, a proven join-semilattice — so ALL of
`SafeMesh.Delta` instantiates at `S := Crdt.RGA.State ι α` for free, and the
ordering obligation is already crdt-lean's: `Crdt.RGA.read_sorted` and
`Crdt.RGA.read_strong_eventual_consistency` hold for ANY state, including the
delta-accumulated one. Nothing about convergence or ordering is re-proved;
identifier allocation stays abstracted (a fresh `ι` is handed in), exactly as
crdt-lean abstracts it. Deltas, single instance on the wire:

- `rgaInsertDelta p v = ({(p, v)}, ∅)` — one positioned element.
- `rgaDeleteDelta p = (∅, {p})` — one tombstoned position.

1. `deltaRGA_placed` / `deltaRGA_tombs` — the accumulation bridges, both
   collapsing concretely by sup-of-singletons (delete deltas ship ONE
   position, so unlike the OR-Set's token-sets both sides collapse): a
   replica that received insert-deltas for exactly `P : Finset (ι × α)` and
   delete-deltas for exactly `T : Finset ι` holds placed-component `P` and
   tombstone-component `T`.
2. `deltaRGA_matches_full` (rfl-grade via `deltaState_eq_replicaState`) with
   `deltaRGA_read_match` (equal ordered read against a full-state replica on
   the same delta-set), and `deltaRGA_read_sec`: two replicas folding ANY
   delivery lists with the same delta-set produce the identical ordered
   sequence (inherited `Crdt.RGA.read_strong_eventual_consistency` —
   `deltaApply` IS `foldr merge ⊥`).
3. `deltaRGA_read_mem` — the live-position read over what was shipped: `p`
   appears in the ordered read iff it was placed by some insert-delta and not
   deleted by any delete-delta, via `Crdt.RGA.mem_read` + `mem_positions` +
   the bridges. `deltaRGA_read_sorted` states the inherited sortedness
   explicitly. (A fully-explicit "read = this sorted list" form was not a
   frozen target and is not attempted; membership + sortedness + read-SEC is
   the target set.)

Scope guard: ONE construction (delta RGA), reusing `SafeMesh.Delta`
generically + the OR-Set technique. No identifier-allocation machinery, no
delta-intervals, no anti-entropy layer.
-/

namespace SafeMesh

open Crdt.RGA (State read positions)

variable {ι α : Type*} [LinearOrder ι] [DecidableEq α]

/-- Insert-delta: one positioned element `(p, v)` on the wire, nothing else. -/
def rgaInsertDelta (p : ι) (v : α) : State ι α := ({(p, v)}, ∅)

/-- Delete-delta: tombstone the single position `p`, nothing else. -/
def rgaDeleteDelta (p : ι) : State ι α := (∅, {p})

/-- The delta-set a mesh node accumulates from placed elements `P` and deleted
positions `T`. -/
def rgaDeltas (P : Finset (ι × α)) (T : Finset ι) : Finset (State ι α) :=
  (P.image fun q => rgaInsertDelta q.1 q.2) ∪ (T.image rgaDeleteDelta)

/-- Componentwise sup bridges for a product carrier (file-private mirrors of
the OR-Set helpers — those are private to their file by design). -/
private theorem fst_deltaState {A B : Type*} [SemilatticeSup A] [OrderBot A]
    [SemilatticeSup B] [OrderBot B] (Δ : Finset (A × B)) :
    (deltaState Δ).1 = Δ.sup Prod.fst := by
  classical
  induction Δ using Finset.induction_on with
  | empty => simp [deltaState]
  | insert q Δ hq ih =>
    simp only [deltaState, Finset.sup_insert, id_eq] at *
    simp [Prod.fst_sup, ih]

private theorem snd_deltaState {A B : Type*} [SemilatticeSup A] [OrderBot A]
    [SemilatticeSup B] [OrderBot B] (Δ : Finset (A × B)) :
    (deltaState Δ).2 = Δ.sup Prod.snd := by
  classical
  induction Δ using Finset.induction_on with
  | empty => simp [deltaState]
  | insert q Δ hq ih =>
    simp only [deltaState, Finset.sup_insert, id_eq] at *
    simp [Prod.snd_sup, ih]

/-- Finset-sup of singletons is the set itself. -/
private theorem sup_singletons {β : Type*} [DecidableEq β] (s : Finset β) :
    s.sup (fun b => ({b} : Finset β)) = s := by
  classical
  induction s using Finset.induction_on with
  | empty => rfl
  | insert b s hb ih => rw [Finset.sup_insert, ih, Finset.sup_eq_union, Finset.insert_eq]

/-- **The accumulated placed-component is exactly the shipped inserts.**
Delete-deltas contribute nothing to it. -/
theorem deltaRGA_placed (P : Finset (ι × α)) (T : Finset ι) :
    (deltaState (rgaDeltas P T)).1 = P := by
  classical
  rw [fst_deltaState, rgaDeltas, Finset.sup_union]
  have hP : (P.image fun q => rgaInsertDelta q.1 q.2).sup Prod.fst = P := by
    rw [Finset.sup_image]
    have : ∀ q : ι × α, (Prod.fst ∘ fun q : ι × α => rgaInsertDelta q.1 q.2) q = {q} := by
      intro q; simp [rgaInsertDelta]
    rw [Finset.sup_congr rfl (fun q _ => this q)]
    exact sup_singletons P
  have hT : (T.image rgaDeleteDelta).sup Prod.fst = (⊥ : Finset (ι × α)) := by
    rw [Finset.sup_image]
    exact Finset.sup_bot _
  rw [hP, hT, sup_bot_eq]

/-- **The accumulated tombstone-component is exactly the shipped deletes.**
Insert-deltas contribute nothing to it. -/
theorem deltaRGA_tombs (P : Finset (ι × α)) (T : Finset ι) :
    (deltaState (rgaDeltas P T)).2 = T := by
  classical
  rw [snd_deltaState, rgaDeltas, Finset.sup_union]
  have hP : (P.image fun q => rgaInsertDelta q.1 q.2).sup Prod.snd = (⊥ : Finset ι) := by
    rw [Finset.sup_image]
    exact Finset.sup_bot _
  have hT : (T.image (rgaDeleteDelta (α := α))).sup Prod.snd = T := by
    rw [Finset.sup_image]
    have : ∀ p : ι, (Prod.snd ∘ rgaDeleteDelta (α := α)) p = ({p} : Finset ι) := by
      intro p; simp [rgaDeleteDelta]
    rw [Finset.sup_congr rfl (fun p _ => this p)]
    exact sup_singletons T
  rw [hP, hT, bot_sup_eq]

/-- A delta RGA replica and a full-state RGA replica fed the same delta-set
hold identical state — `delta_matches_state` instantiated. -/
theorem deltaRGA_matches_full (Δ : Finset (State ι α)) :
    deltaState Δ = Crdt.replicaState Δ :=
  deltaState_eq_replicaState _

/-- Equal state, equal ordered read against the full-state replica. -/
theorem deltaRGA_read_match (Δ : Finset (State ι α)) :
    read (deltaState Δ) = read (Crdt.replicaState Δ) :=
  congrArg read (deltaRGA_matches_full Δ)

/-- **Sequence-level SEC for delta dissemination.** Two replicas folding ANY
delivery lists with the same underlying delta-set produce the identical
ordered sequence — the same list, in the same order. Inherited from
`Crdt.RGA.read_strong_eventual_consistency` (`deltaApply` is the same fold). -/
theorem deltaRGA_read_sec (l₁ l₂ : List (State ι α))
    (h : l₁.toFinset = l₂.toFinset) :
    read (deltaApply l₁) = read (deltaApply l₂) :=
  Crdt.RGA.read_strong_eventual_consistency l₁ l₂ h

/-- **The ordered read over what was shipped.** A position appears in the read
iff some insert-delta placed it and no delete-delta tombstoned it. -/
theorem deltaRGA_read_mem (P : Finset (ι × α)) (T : Finset ι) (p : ι) :
    p ∈ read (deltaState (rgaDeltas P T)) ↔ (∃ v, (p, v) ∈ P) ∧ p ∉ T := by
  rw [Crdt.RGA.mem_read, Crdt.RGA.mem_positions, deltaRGA_placed, deltaRGA_tombs]
  constructor
  · rintro ⟨v, hv, hp⟩; exact ⟨⟨v, hv⟩, hp⟩
  · rintro ⟨⟨v, hv⟩, hp⟩; exact ⟨v, hv, hp⟩

/-- The delta-accumulated read is sorted by position identifier — the
inherited ordering obligation (`Crdt.RGA.read_sorted`), stated explicitly for
the delta replica: sequence order is a function of live positions, never of
delta delivery history. -/
theorem deltaRGA_read_sorted (Δ : Finset (State ι α)) :
    (read (deltaState Δ)).Pairwise (· ≤ ·) :=
  Crdt.RGA.read_sorted _

end SafeMesh
