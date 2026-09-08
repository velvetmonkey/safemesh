export type ORSetElement = string
export type ORSetToken = string

export type ORSetAdd = {
  kind: 'orset.add'
  element: ORSetElement
  token: ORSetToken
}

export type ORSetRemove = {
  kind: 'orset.remove'
  tokens: ORSetToken[]
}

export type ORSetDelta = ORSetAdd | ORSetRemove

export type ORSetState = {
  adds: Record<ORSetToken, ORSetElement>
  tombstones: Record<ORSetToken, true>
}

export function bottomORSet(): ORSetState {
  return { adds: {}, tombstones: {} }
}

// Mirrors SafeMesh.orAddDelta and deltaORSet_lookup.
// Historical TypeScript reference, retained for its own tests only.
// The Lab uses Rust-core records and reads through safemesh-wasm.
export function addDelta(element: ORSetElement, token: ORSetToken): ORSetDelta {
  return { kind: 'orset.add', element, token }
}

// Mirrors SafeMesh.orRemoveDelta: remove only tombstones observed add tokens.
export function removeDelta(tokens: ORSetToken[]): ORSetDelta {
  return { kind: 'orset.remove', tokens: [...new Set(tokens)].sort() }
}

// Mirrors deltaORSet_adds / deltaORSet_tombs: merge is union of add instances
// and union of shipped tombstone sets, so delivery order and redelivery vanish.
export function applyORSetDelta(state: ORSetState, delta: ORSetDelta): ORSetState {
  if (delta.kind === 'orset.add') {
    return {
      adds: { ...state.adds, [delta.token]: delta.element },
      tombstones: { ...state.tombstones },
    }
  }

  const tombstones = { ...state.tombstones }
  for (const token of delta.tokens) {
    tombstones[token] = true
  }
  return { adds: { ...state.adds }, tombstones }
}

export function mergeORSet(left: ORSetState, right: ORSetState): ORSetState {
  return {
    adds: { ...left.adds, ...right.adds },
    tombstones: { ...left.tombstones, ...right.tombstones },
  }
}

// Mirrors SafeMesh.deltaORSet_lookup: an element is visible if at least one
// shipped add instance for it has a token outside the accumulated tombstones.
export function readORSet(state: ORSetState): ORSetElement[] {
  const visible = new Set<ORSetElement>()
  for (const [token, element] of Object.entries(state.adds)) {
    if (!state.tombstones[token]) {
      visible.add(element)
    }
  }
  return [...visible].sort((a, b) => a.localeCompare(b))
}

export function observedTokens(state: ORSetState, element: ORSetElement): ORSetToken[] {
  return Object.entries(state.adds)
    .filter(([token, value]) => value === element && !state.tombstones[token])
    .map(([token]) => token)
    .sort()
}

export function sameORSet(left: ORSetState, right: ORSetState): boolean {
  return (
    JSON.stringify(sortedRecord(left.adds)) === JSON.stringify(sortedRecord(right.adds)) &&
    JSON.stringify(sortedRecord(left.tombstones)) === JSON.stringify(sortedRecord(right.tombstones))
  )
}

function sortedRecord<T>(record: Record<string, T>): Record<string, T> {
  return Object.fromEntries(Object.entries(record).sort(([a], [b]) => a.localeCompare(b)))
}
