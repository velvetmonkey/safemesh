/-
SafeMesh — delta-state CRDT convergence, built on crdt-lean.
Copyright (C) 2026 Ben Cassie
SPDX-License-Identifier: BUSL-1.1
-/
import SafeMesh.DeltaGCounter

/-!
# The delta PN-Counter

**Frozen targets (STEP-0, written before proving):**

The PN-Counter (`Crdt.PNCounter ι = (ι → ℕ) × (ι → ℕ)`, Prod lattice) is a
pair of G-Counters: increments `P`, decrements `N`. Its deltas are therefore
single-coordinate bumps on ONE side, `⊥` on the other — one coordinate on the
wire, exactly like the delta G-Counter:

- `deltaBumpP r v = (Pi.single r v, ⊥)` — replica `r` asserts increment tally `v`.
- `deltaBumpN r v = (⊥, Pi.single r v)` — replica `r` asserts decrement tally `v`.

1. `deltaPNCounter_correct_P` / `deltaPNCounter_correct_N` — the state-level
   result: pointwise on each side, the delta-disseminated `P` (resp. `N`)
   component over bump-sets `P N : Finset (ι × ℕ)` equals the per-coordinate
   max over that coordinate's own P-bumps (resp. N-bumps). Proved by reducing
   each Prod component to `deltaGCounter_correct` via the componentwise
   `Finset.sup` bridge (`deltaState_fst` / `deltaState_snd` — the PN analogue
   of `deltaState_apply`, and the real work here).
2. `deltaPNCounter_matches_full` — a delta PN replica and a full-state PN
   replica fed the same bumps hold identical state (rfl-grade via
   `deltaState_eq_replicaState`), hence equal reads:
   `deltaPNCounter_value_matches` for `Crdt.pncounterValue`.

Scope guard: ONE construction (delta PN-Counter), reusing the delta G-Counter
componentwise. No OR-Set, no delta-intervals, no anti-entropy layer.
-/

namespace SafeMesh

open Crdt (PNCounter GCounter)

variable {ι : Type*} [Fintype ι] [DecidableEq ι]

/-- Increment delta: replica `r` asserts increment tally `v`. `P`-side single
coordinate, `N`-side `⊥`. -/
def deltaBumpP (r : ι) (v : ℕ) : PNCounter ι := (Pi.single r v, ⊥)

/-- Decrement delta: replica `r` asserts decrement tally `v`. `N`-side single
coordinate, `P`-side `⊥`. -/
def deltaBumpN (r : ι) (v : ℕ) : PNCounter ι := (⊥, Pi.single r v)

omit [DecidableEq ι] in
/-- Componentwise `Finset.sup` bridge, `P` side: the first component of a
delta-PN replica's state is the join of the first components. The PN analogue
of `deltaState_apply`. -/
theorem deltaState_fst (Δ : Finset (PNCounter ι)) :
    (deltaState Δ).1 = Δ.sup Prod.fst := by
  classical
  induction Δ using Finset.induction_on with
  | empty => simp [deltaState]
  | insert p Δ hp ih =>
    simp only [deltaState, Finset.sup_insert, id_eq] at *
    simp [Prod.fst_sup, ih]

omit [DecidableEq ι] in
/-- Componentwise `Finset.sup` bridge, `N` side. -/
theorem deltaState_snd (Δ : Finset (PNCounter ι)) :
    (deltaState Δ).2 = Δ.sup Prod.snd := by
  classical
  induction Δ using Finset.induction_on with
  | empty => simp [deltaState]
  | insert p Δ hp ih =>
    simp only [deltaState, Finset.sup_insert, id_eq] at *
    simp [Prod.snd_sup, ih]

/-- The delta-set a mesh node accumulates from increment bumps `P` and
decrement bumps `N`. -/
def pnDeltas (P N : Finset (ι × ℕ)) : Finset (PNCounter ι) :=
  (P.image fun p => deltaBumpP p.1 p.2) ∪ (N.image fun p => deltaBumpN p.1 p.2)

/-- **The delta PN-Counter's `P` side is the delta G-Counter.** Pointwise, the
disseminated increment component equals the per-coordinate max over that
coordinate's own increment bumps; decrement deltas contribute nothing to it. -/
theorem deltaPNCounter_correct_P (P N : Finset (ι × ℕ)) (i : ι) :
    (deltaState (pnDeltas P N)).1 i = (P.filter fun p => p.1 = i).sup Prod.snd := by
  classical
  have hP : ((P.image fun p => deltaBumpP p.1 p.2).sup Prod.fst)
      = deltaState (P.image fun p => deltaBump p.1 p.2) := by
    rw [Finset.sup_image, deltaState, Finset.sup_image]
    rfl
  have hN : ((N.image fun p => deltaBumpN p.1 p.2).sup Prod.fst) = (⊥ : GCounter ι) := by
    rw [Finset.sup_image]
    exact Finset.sup_bot _
  rw [deltaState_fst, pnDeltas, Finset.sup_union, hP, hN, sup_bot_eq]
  exact deltaGCounter_correct P i

/-- **The delta PN-Counter's `N` side is the delta G-Counter.** Symmetric to
`deltaPNCounter_correct_P`. -/
theorem deltaPNCounter_correct_N (P N : Finset (ι × ℕ)) (i : ι) :
    (deltaState (pnDeltas P N)).2 i = (N.filter fun p => p.1 = i).sup Prod.snd := by
  classical
  have hP : ((P.image fun p => deltaBumpP p.1 p.2).sup Prod.snd) = (⊥ : GCounter ι) := by
    rw [Finset.sup_image]
    exact Finset.sup_bot _
  have hN : ((N.image fun p => deltaBumpN p.1 p.2).sup Prod.snd)
      = deltaState (N.image fun p => deltaBump p.1 p.2) := by
    rw [Finset.sup_image, deltaState, Finset.sup_image]
    rfl
  rw [deltaState_snd, pnDeltas, Finset.sup_union, hP, hN, bot_sup_eq]
  exact deltaGCounter_correct N i

/-- A delta PN replica and a full-state PN replica fed the same bumps hold
identical state — the rfl-grade instantiation of `delta_matches_state`. -/
theorem deltaPNCounter_matches_full (P N : Finset (ι × ℕ)) :
    deltaState (pnDeltas P N) = Crdt.replicaState (pnDeltas P N) :=
  deltaState_eq_replicaState _

/-- Equal state, equal read: the delta PN replica's counter value is the
full-state replica's counter value. -/
theorem deltaPNCounter_value_matches (P N : Finset (ι × ℕ)) :
    Crdt.pncounterValue (deltaState (pnDeltas P N))
      = Crdt.pncounterValue (Crdt.replicaState (pnDeltas P N)) :=
  congrArg Crdt.pncounterValue (deltaPNCounter_matches_full P N)

end SafeMesh
