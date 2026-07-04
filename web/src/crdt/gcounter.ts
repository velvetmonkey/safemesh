export type ReplicaId = number

export type GCounterDelta = {
  kind: 'gcounter.bump'
  replica: ReplicaId
  tally: number
}

export type GCounterState = number[]

export function bottomGCounter(replicas: number): GCounterState {
  return Array.from({ length: replicas }, () => 0)
}

// Mirrors SafeMesh.deltaBump / SafeMesh.deltaGCounter_correct:
// a bump is one replica coordinate asserting its current grow-only tally.
export function bumpDelta(replica: ReplicaId, tally: number): GCounterDelta {
  return { kind: 'gcounter.bump', replica, tally }
}

// Mirrors SafeMesh.delta_dissemination_sec and merge_deltaState:
// applying a delta is joining it into the carrier, here pointwise max.
export function applyGCounterDelta(
  state: GCounterState,
  delta: GCounterDelta,
): GCounterState {
  const next = [...state]
  if (delta.replica >= 0 && delta.replica < next.length) {
    next[delta.replica] = Math.max(next[delta.replica], delta.tally)
  }
  return next
}

// Mirrors the full-state G-Counter join used by Crdt.merge.
export function mergeGCounter(left: GCounterState, right: GCounterState): GCounterState {
  return left.map((value, index) => Math.max(value, right[index] ?? 0))
}

// Mirrors Crdt.gcounterValue.
export function readGCounter(state: GCounterState): number {
  return state.reduce((sum, value) => sum + value, 0)
}

export function sameGCounter(left: GCounterState, right: GCounterState): boolean {
  return left.length === right.length && left.every((value, index) => value === right[index])
}
