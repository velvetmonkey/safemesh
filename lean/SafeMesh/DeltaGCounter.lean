/-
SafeMesh — delta-state CRDT convergence, built on crdt-lean.
Copyright (C) 2026 Ben Cassie
SPDX-License-Identifier: AGPL-3.0-or-later
-/
import SafeMesh.Delta
import Crdt.Instances

/-!
# The delta G-Counter

**Frozen target (STEP-0, written before proving):**

3. `deltaGCounter_correct` — the concrete anchor. A bump `(r, v)` (replica `r`
   asserting tally `v`) ships as the single-coordinate delta
   `deltaBump r v = Pi.single r v : Crdt.GCounter ι` — `v` at coordinate `r`,
   `⊥` everywhere else. That is the whole bandwidth story: one coordinate on
   the wire instead of the full `ι → ℕ` vector. The theorem: pointwise, the
   delta-disseminated state equals the per-coordinate max of that coordinate's
   own bumps —
   `deltaState (B.image fun p => deltaBump p.1 p.2) i
      = (B.filter fun p => p.1 = i).sup Prod.snd`
   — exactly the join the full-state G-Counter (`Crdt.GCounter`, pointwise max
   merge) holds. Corollary `deltaGCounter_matches_full`: a delta replica and a
   full-state replica fed the same bumps hold identical state.

Scope guard: this is the ONE concrete construction. No PN-Counter delta, no
OR-Set delta, no delta-intervals, no anti-entropy layer.
-/

namespace SafeMesh

open Crdt (GCounter)

variable {ι : Type*} [Fintype ι] [DecidableEq ι]

/-- The delta a replica ships for the bump "replica `r`'s tally is now `v`":
`v` at coordinate `r`, `⊥ = 0` everywhere else. One coordinate on the wire. -/
def deltaBump (r : ι) (v : ℕ) : GCounter ι := Pi.single r v

omit [Fintype ι] in
@[simp] theorem deltaBump_apply (r : ι) (v : ℕ) (i : ι) :
    deltaBump r v i = if i = r then v else 0 :=
  Pi.single_apply r v i

omit [Fintype ι] [DecidableEq ι] in
/-- Pointwise reading of a delta-replica's state: the join distributes over
coordinates (Pi lattice is pointwise). -/
theorem deltaState_apply (Δ : Finset (GCounter ι)) (i : ι) :
    deltaState Δ i = Δ.sup (fun d => d i) := by
  simp [deltaState, Finset.sup_apply]

/-- **The delta G-Counter is the G-Counter.** Pointwise, the state a replica
reaches by joining single-coordinate bump deltas equals the max over that
coordinate's own bumps — the same join the full-state G-Counter holds after
merging full vectors carrying those tallies. -/
theorem deltaGCounter_correct (B : Finset (ι × ℕ)) (i : ι) :
    deltaState (B.image fun p => deltaBump p.1 p.2) i
      = (B.filter fun p => p.1 = i).sup Prod.snd := by
  classical
  rw [deltaState_apply, Finset.sup_image]
  induction B using Finset.induction_on with
  | empty => simp
  | insert p B hp ih =>
    rw [Finset.sup_insert, ih, Finset.filter_insert, Function.comp_apply, deltaBump_apply]
    by_cases h : p.1 = i
    · rw [if_pos h, if_pos h.symm, Finset.sup_insert]
    · rw [if_neg h, if_neg (fun hip => h hip.symm), ← Nat.bot_eq_zero, bot_sup_eq]

/-- A delta replica and a full-state replica fed the same bumps hold identical
state — the rfl-grade instantiation of `delta_matches_state` at the G-Counter,
stated so the anchor is explicit. -/
theorem deltaGCounter_matches_full (B : Finset (ι × ℕ)) :
    deltaState (B.image fun p => deltaBump p.1 p.2)
      = Crdt.replicaState (B.image fun p => deltaBump p.1 p.2) :=
  deltaState_eq_replicaState _

end SafeMesh
