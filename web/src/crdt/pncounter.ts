import {
  applyGCounterDelta,
  bottomGCounter,
  bumpDelta,
  mergeGCounter,
  readGCounter,
  type GCounterState,
  type ReplicaId,
} from './gcounter'

export type PNCounterDelta =
  | { kind: 'pncounter.inc'; replica: ReplicaId; tally: number }
  | { kind: 'pncounter.dec'; replica: ReplicaId; tally: number }

export type PNCounterState = {
  p: GCounterState
  n: GCounterState
}

export function bottomPNCounter(replicas: number): PNCounterState {
  return { p: bottomGCounter(replicas), n: bottomGCounter(replicas) }
}

// Mirrors SafeMesh.deltaBumpP / deltaBumpN and
// SafeMesh.deltaPNCounter_correct_P / _N. Rust ships this CRDT today, but the
// PWA uses it only as a reference mirror unless a panel is added later.
export function applyPNCounterDelta(
  state: PNCounterState,
  delta: PNCounterDelta,
): PNCounterState {
  if (delta.kind === 'pncounter.inc') {
    return { p: applyGCounterDelta(state.p, bumpDelta(delta.replica, delta.tally)), n: state.n }
  }
  return { p: state.p, n: applyGCounterDelta(state.n, bumpDelta(delta.replica, delta.tally)) }
}

export function mergePNCounter(left: PNCounterState, right: PNCounterState): PNCounterState {
  return { p: mergeGCounter(left.p, right.p), n: mergeGCounter(left.n, right.n) }
}

// Mirrors SafeMesh.deltaPNCounter_value_matches / Crdt.pncounterValue.
export function readPNCounter(state: PNCounterState): number {
  return readGCounter(state.p) - readGCounter(state.n)
}
