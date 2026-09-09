import { SafeMeshGCounterReplica, SafeMeshStringOrSetReplica } from '../../../rust/crates/safemesh-wasm/pkg/safemesh_wasm'

export type GCounterDelta = {
  kind: 'gcounter.bump'
  replica: number
  tally: number
  bytes: Uint8Array
  payload: 'record' | 'log'
}

// Labels are presentation metadata; only bytes are admitted to the core.
export type ORSetDelta = (
  | { kind: 'orset.add'; element: string; token: bigint }
  | { kind: 'orset.remove'; tokens: bigint[] }
  | { kind: 'orset.log' }
) & { bytes: Uint8Array; payload: 'record' | 'log' }

export type MeshDelta = GCounterDelta | ORSetDelta

export type Peer = {
  id: number
  // This is a read-only Rust-core snapshot for the UI; `gcounterLog` is the carrier.
  gcounter: number[]
  gcounterLog: Uint8Array
  // Immutable core log, reconstructed into a WASM handle for each operation.
  orset: Uint8Array
  localTally: number
}

export type Packet = {
  id: string
  from: number
  to: number
  delta: MeshDelta
  sentAt: number
  deliverAt: number
  duplicated?: boolean
  phase?: 'normal' | 'reordered' | 'repair'
  order?: number
}

export type LogEntry = {
  id: string
  at: number
  plain: string
  technical: string
  tone: 'send' | 'deliver' | 'drop' | 'partition' | 'merge' | 'anti-entropy'
}

export type Simulation = {
  peers: Peer[]
  queue: Packet[]
  partitioned: boolean
  latencyMs: number
  dropRate: number
  antiEntropyMs: number
  nextAntiEntropyAt: number
  lastAntiEntropyAt: number | null
  now: number
  nextId: number
  log: LogEntry[]
}

export function createSimulation(peerCount = 4): Simulation {
  return {
    peers: Array.from({ length: peerCount }, (_, id) => ({
      id,
      ...emptyGCounterPeer(id, peerCount),
      orset: new Uint8Array(),
      localTally: 0,
    })),
    queue: [],
    partitioned: false,
    latencyMs: 550,
    dropRate: 0.08,
    antiEntropyMs: 5000,
    nextAntiEntropyAt: 5000,
    lastAntiEntropyAt: null,
    now: 0,
    nextId: 1,
    log: [],
  }
}

export function setPeerCount(sim: Simulation, peerCount: number): Simulation {
  const next = createSimulation(peerCount)
  return {
    ...next,
    latencyMs: sim.latencyMs,
    dropRate: sim.dropRate,
    antiEntropyMs: sim.antiEntropyMs,
    nextAntiEntropyAt: sim.antiEntropyMs,
    lastAntiEntropyAt: null,
    log: appendLog(next, `reset mesh to ${peerCount} peers`, 'partition').log,
  }
}

export function setPartitioned(sim: Simulation, partitioned: boolean): Simulation {
  return appendLog(
    { ...sim, partitioned, nextAntiEntropyAt: partitioned ? sim.nextAntiEntropyAt : sim.now },
    partitioned ? 'Network cut: new radio messages are stuck locally' : 'Network back up: camps catching up',
    'partition',
    partitioned ? 'partition enabled: generated deltas stay local' : 'partition healed: queued gossip resumes',
  )
}

export function setLatency(sim: Simulation, latencyMs: number): Simulation {
  return { ...sim, latencyMs }
}

export function setDropRate(sim: Simulation, dropRate: number): Simulation {
  return { ...sim, dropRate }
}

export function setAntiEntropyMs(sim: Simulation, antiEntropyMs: number): Simulation {
  return {
    ...sim,
    antiEntropyMs,
    nextAntiEntropyAt: antiEntropyMs > 0 ? sim.now + antiEntropyMs : Number.POSITIVE_INFINITY,
  }
}

export function bumpCounter(sim: Simulation, peerId: number): Simulation {
  const peer = sim.peers[peerId]
  if (!peer) return sim
  const tally = peer.localTally + 1
  const replica = replicaFor(peer, sim.peers.length)
  const bytes = replica.appendBump(peerId, BigInt(tally))
  return emitDelta(
    sim,
    peerId,
    { kind: 'gcounter.bump', replica: peerId, tally, bytes, payload: 'record' },
    `Camp ${peerId} increases headcount to ${tally}`,
    `peer ${peerId} bumps G-Counter to ${tally}`,
  )
}

export function addElement(sim: Simulation, peerId: number, element: string): Simulation {
  const peer = sim.peers[peerId]
  const clean = element.trim()
  if (!peer || clean.length === 0) return sim
  const token = BigInt(sim.nextId)
  const replica = orsetReplicaFor(peer)
  let bytes: Uint8Array
  try {
    bytes = replica.appendAdd(clean, token)
  } finally {
    replica.free()
  }
  return emitDelta(
    sim,
    peerId,
    { kind: 'orset.add', element: clean, token, bytes, payload: 'record' },
    `Camp ${peerId} added '${clean}' to the supply list`,
    `peer ${peerId} adds "${clean}" (${token})`,
  )
}

export function removeElement(sim: Simulation, peerId: number, element: string): Simulation {
  const peer = sim.peers[peerId]
  if (!peer) return sim
  const replica = orsetReplicaFor(peer)
  const tokens = Array.from(replica.observedTokens(element))
  if (tokens.length === 0) {
    replica.free()
    return appendLog(
      sim,
      `Camp ${peerId} cannot remove '${element}' because it has not seen it`,
      'drop',
      `peer ${peerId} remove skipped; "${element}" is not observed`,
    )
  }
  let bytes: Uint8Array
  try {
    bytes = replica.appendRemoveObserved(element)
  } finally {
    replica.free()
  }
  return emitDelta(
    sim,
    peerId,
    { kind: 'orset.remove', tokens, bytes, payload: 'record' },
    `Camp ${peerId} removed the '${element}' supplies it had seen`,
    `peer ${peerId} removes observed "${element}" tokens [${tokens.join(', ')}]`,
  )
}

export function tick(sim: Simulation, elapsedMs: number, random = Math.random): Simulation {
  const now = sim.now + elapsedMs
  const due = sim.queue.filter((packet) => packet.deliverAt <= now)
  const pending = sim.queue.filter((packet) => packet.deliverAt > now)
  let next: Simulation = { ...sim, now, queue: pending }

  for (const packet of due) {
    if (sim.partitioned) {
      next = {
        ...next,
        queue: [...next.queue, { ...packet, sentAt: now, deliverAt: now + sim.latencyMs }],
      }
      continue
    }

    if (random() < sim.dropRate) {
      next = appendLog(
        next,
        plainDrop(packet),
        'drop',
        `dropped ${describeDelta(packet.delta)} ${packet.from}->${packet.to}`,
      )
      continue
    }

    next = deliverPacket(next, packet)
  }

  return maybeRunAntiEntropy(next)
}

export function convergence(sim: Simulation): {
  converged: boolean
  sameRawState: boolean
  sameReads: boolean
  // Per peer, against the first replica: the Rust-core carrier comparison, and the displayed-read comparison.
  rawStateMatches: boolean[]
  readMatches: boolean[]
  gcounterValue: number
  gcounterValues: number[]
  orsetElements: string[]
} {
  const [first] = sim.peers
  if (!first) {
    return { converged: true, sameRawState: true, sameReads: true, rawStateMatches: [], readMatches: [], gcounterValue: 0, gcounterValues: [], orsetElements: [] }
  }

  const firstReplica = replicaFor(first, sim.peers.length)
  const rawStateMatches = sim.peers.map(
    (peer) => replicaFor(peer, sim.peers.length).sameStateAs(firstReplica) && orsetStateKey(peer) === orsetStateKey(first),
  )
  const sameRawState = rawStateMatches.every(Boolean)
  const firstGRead = Number(firstReplica.value())
  const firstORead = JSON.stringify(readORSet(first.orset))
  const readMatches = sim.peers.map(
    (peer) => Number(replicaFor(peer, sim.peers.length).value()) === firstGRead && JSON.stringify(readORSet(peer.orset)) === firstORead,
  )
  const sameReads = readMatches.every(Boolean)

  return {
    converged: sameRawState && sim.queue.length === 0,
    sameRawState,
    sameReads,
    rawStateMatches,
    readMatches,
    gcounterValue: firstGRead,
    gcounterValues: sim.peers.map((peer) => Number(replicaFor(peer, sim.peers.length).value())),
    orsetElements: readORSet(first.orset),
  }
}

export function runAntiEntropyNow(sim: Simulation): Simulation {
  return runAntiEntropy({ ...sim, nextAntiEntropyAt: sim.now })
}

export function queueAntiEntropyPackets(sim: Simulation): Simulation {
  if (sim.partitioned) {
    return appendLog(
      sim,
      'Anti-entropy cannot cross the partition yet',
      'partition',
      'anti-entropy skipped: partition still enabled',
    )
  }

  const { packets, nextId } = buildAntiEntropyPackets(sim)
  const nextBase = {
    ...sim,
    nextId,
    nextAntiEntropyAt: sim.antiEntropyMs > 0 ? sim.now + sim.antiEntropyMs : Number.POSITIVE_INFINITY,
    lastAntiEntropyAt: packets.length > 0 ? sim.now : sim.lastAntiEntropyAt,
  }

  if (packets.length === 0) {
    return appendLog(nextBase, 'Anti-entropy checked: no missing modeled records', 'anti-entropy')
  }

  return appendLog(
    {
      ...nextBase,
      queue: [...sim.queue, ...packets],
    },
    `Anti-entropy queued ${packets.length} visible repair ${packets.length === 1 ? 'message' : 'messages'}`,
    'anti-entropy',
    `anti-entropy queued ${packets.length} repair deltas`,
  )
}

export function dropNextPacket(sim: Simulation): Simulation {
  const index = nextPacketIndex(sim.queue)
  if (index < 0) {
    return appendLog(sim, 'No queued delta to drop', 'drop', 'drop requested: queue empty')
  }
  return dropPacketAtIndex(sim, index)
}

export function dropCounterPacketToPeer(sim: Simulation, peerId: number): Simulation {
  const index = sim.queue.findIndex((packet) => packet.to === peerId && packet.delta.kind === 'gcounter.bump')
  if (index < 0) return dropNextPacket(sim)
  return dropPacketAtIndex(sim, index)
}

function dropPacketAtIndex(sim: Simulation, index: number): Simulation {
  const packet = sim.queue[index]
  return appendLog(
    { ...sim, queue: sim.queue.filter((_, packetIndex) => packetIndex !== index) },
    plainDrop(packet),
    'drop',
    `operator dropped ${describeDelta(packet.delta)} ${packet.from}->${packet.to}`,
  )
}

export function duplicateNextPacket(sim: Simulation): Simulation {
  const index = nextPacketIndex(sim.queue)
  if (index < 0) {
    return appendLog(sim, 'No queued delta to duplicate', 'drop', 'duplicate requested: queue empty')
  }
  const packet = sim.queue[index]
  const duplicate = {
    ...packet,
    id: `${packet.id}-dup-${sim.nextId}`,
    sentAt: sim.now,
    deliverAt: packet.deliverAt + 220,
    duplicated: true,
  }
  return appendLog(
    {
      ...sim,
      queue: [...sim.queue, duplicate],
      nextId: sim.nextId + 1,
    },
    `Duplicated a delta from Camp ${packet.from} to Camp ${packet.to}`,
    'send',
    `duplicated ${describeDelta(packet.delta)} ${packet.from}->${packet.to}`,
  )
}

export function reorderQueue(sim: Simulation): Simulation {
  if (sim.queue.length < 2) {
    return appendLog(sim, 'Need at least two queued deltas to reorder', 'drop', 'reorder requested: queue too small')
  }
  const reordered = [...sim.queue]
    .sort((left, right) => left.deliverAt - right.deliverAt)
    .reverse()
    .map((packet, index) => ({
      ...packet,
      sentAt: sim.now,
      deliverAt: sim.now + 380 + index * 230,
      phase: 'reordered' as const,
      order: index + 1,
    }))
  return appendLog(
    { ...sim, queue: reordered },
    'Reordered queued radio deltas',
    'partition',
    `operator reordered ${reordered.length} queued deltas`,
  )
}

export function wirePreview(delta: MeshDelta): string {
  return Array.from(delta.bytes, (byte) => byte.toString(16).padStart(2, '0').toUpperCase()).join(' ')
}

function emitDelta(
  sim: Simulation,
  peerId: number,
  delta: MeshDelta,
  plainMessage: string,
  technicalMessage: string,
): Simulation {
  const local = applyDeltaToPeer(sim.peers[peerId], delta)
  const peers = sim.peers.map((peer) => (peer.id === peerId ? local : peer))
  const packetBase = sim.nextId
  const packets = sim.peers
    .filter((peer) => peer.id !== peerId)
    .map((peer, index) => ({
      id: `m${packetBase}-${index}`,
      from: peerId,
      to: peer.id,
      delta,
      sentAt: sim.now,
      deliverAt: sim.now + sim.latencyMs + index * 90,
      phase: 'normal' as const,
    }))

  return appendLog(
    {
      ...sim,
      peers,
      queue: [...sim.queue, ...packets],
      nextId: sim.nextId + 1,
    },
    sim.partitioned
      ? `${plainMessage} - stuck until reconnect`
      : `${plainMessage} - sent to ${packets.length} camps`,
    'send',
    sim.partitioned ? `${technicalMessage}; held by partition` : `${technicalMessage}; queued to ${packets.length} peers`,
  )
}

function deliverPacket(sim: Simulation, packet: Packet): Simulation {
  const peers = sim.peers.map((peer) => (peer.id === packet.to ? applyDeltaToPeer(peer, packet.delta) : peer))
  const tone = packet.phase === 'repair' ? 'anti-entropy' : 'deliver'
  return appendLog(
    { ...sim, peers },
    packet.phase === 'repair' ? plainRepair(packet) : plainDeliver(packet),
    tone,
    packet.phase === 'repair'
      ? `anti-entropy delivered ${describeDelta(packet.delta)} ${packet.from}->${packet.to}`
      : `delivered ${describeDelta(packet.delta)} ${packet.from}->${packet.to}`,
  )
}

function applyDeltaToPeer(peer: Peer, delta: MeshDelta): Peer {
  if (delta.kind === 'gcounter.bump') {
    const counter = replicaFor(peer, peer.gcounter.length)
    if (delta.payload === 'record') counter.mergeRecordBytes(delta.bytes)
    else counter.mergeLogBytes(delta.bytes)
    return snapshotGCounterPeer(peer, counter)
  }
  const replica = orsetReplicaFor(peer)
  try {
    if (delta.payload === 'record') replica.mergeRecordBytes(delta.bytes)
    else replica.mergeLogBytes(delta.bytes)
    return { ...peer, orset: replica.logBytes() }
  } finally {
    replica.free()
  }
}

function maybeRunAntiEntropy(sim: Simulation): Simulation {
  if (sim.antiEntropyMs <= 0 || sim.now < sim.nextAntiEntropyAt) return sim
  if (sim.partitioned) return sim
  return runAntiEntropy(sim)
}

function runAntiEntropy(sim: Simulation): Simulation {
  if (sim.partitioned || sim.antiEntropyMs <= 0) return sim

  let peers = sim.peers
  let next: Simulation = { ...sim, nextAntiEntropyAt: sim.now + sim.antiEntropyMs }
  let mergedAny = false

  for (let i = 0; i < peers.length; i += 1) {
    for (let j = i + 1; j < peers.length; j += 1) {
      const left = peers[i]
      const right = peers[j]
      const rightToLeft = backfillDescriptions(left, right)
      const leftToRight = backfillDescriptions(right, left)

      if (rightToLeft.length === 0 && leftToRight.length === 0) continue
      mergedAny = true

      const leftCounter = replicaFor(left, peers.length)
      const rightCounter = replicaFor(right, peers.length)
      leftCounter.mergeLogBytes(rightCounter.logBytes())
      rightCounter.mergeLogBytes(leftCounter.logBytes())
      const leftSet = orsetReplicaFor(left)
      const rightSet = orsetReplicaFor(right)
      let leftLog: Uint8Array
      let rightLog: Uint8Array
      try {
        leftSet.mergeLogBytes(rightSet.logBytes())
        rightSet.mergeLogBytes(leftSet.logBytes())
        leftLog = leftSet.logBytes()
        rightLog = rightSet.logBytes()
      } finally {
        leftSet.free()
        rightSet.free()
      }
      peers = peers.map((peer) => {
        if (peer.id === left.id) return normalizePeer({ ...snapshotGCounterPeer(left, leftCounter), orset: leftLog })
        if (peer.id === right.id) return normalizePeer({ ...snapshotGCounterPeer(right, rightCounter), orset: rightLog })
        return peer
      })

      for (const item of rightToLeft) {
        next = appendLog(
          { ...next, peers },
          `Camp ${left.id} recovered ${item.plain} from Camp ${right.id}`,
          'anti-entropy',
          `anti-entropy: peer ${left.id} backfilled ${item.technical} from peer ${right.id}`,
        )
      }
      for (const item of leftToRight) {
        next = appendLog(
          { ...next, peers },
          `Camp ${right.id} recovered ${item.plain} from Camp ${left.id}`,
          'anti-entropy',
          `anti-entropy: peer ${right.id} backfilled ${item.technical} from peer ${left.id}`,
        )
      }
    }
  }

  return {
    ...next,
    peers,
    lastAntiEntropyAt: mergedAny ? sim.now : sim.lastAntiEntropyAt,
  }
}

function nextPacketIndex(queue: Packet[]): number {
  if (queue.length === 0) return -1
  let best = 0
  for (let index = 1; index < queue.length; index += 1) {
    if (queue[index].deliverAt < queue[best].deliverAt) best = index
  }
  return best
}

function buildAntiEntropyPackets(sim: Simulation): { packets: Packet[]; nextId: number } {
  let nextId = sim.nextId
  let repairIndex = 0
  const packets: Packet[] = []

  for (const target of sim.peers) {
    for (const source of sim.peers) {
      if (source.id === target.id) continue
      const sourceCounter = replicaFor(source, sim.peers.length)
      const targetCounter = replicaFor(target, sim.peers.length)
      const hasMissingRecords = Array.from({ length: sim.peers.length }, (_, replica) =>
        sourceCounter.versionFor(BigInt(replica)) > targetCounter.versionFor(BigInt(replica)),
      ).some(Boolean)
      if (!hasMissingRecords) continue
      packets.push({
        id: `r${nextId}-${repairIndex}`,
        from: source.id,
        to: target.id,
        delta: {
          kind: 'gcounter.bump',
          replica: source.id,
          tally: Number(sourceCounter.value()),
          bytes: sourceCounter.logBytes(),
          payload: 'log',
        },
        sentAt: sim.now,
        deliverAt: sim.now + sim.latencyMs + 160 + repairIndex * 210,
        phase: 'repair',
      })
      nextId += 1
      repairIndex += 1
    }

    for (const source of sim.peers) {
      if (source.id === target.id || !missingOrSetRecords(target, source, sim.peers.length)) continue
      const replica = orsetReplicaFor(source)
      let bytes: Uint8Array
      try {
        bytes = replica.logBytes()
      } finally {
        replica.free()
      }
      packets.push({
        id: `r${nextId}-${repairIndex}`,
        from: source.id,
        to: target.id,
        delta: { kind: 'orset.log', bytes, payload: 'log' },
        sentAt: sim.now,
        deliverAt: sim.now + sim.latencyMs + 160 + repairIndex * 210,
        phase: 'repair',
      })
      nextId += 1
      repairIndex += 1
    }
  }

  return { packets, nextId }
}

function backfillDescriptions(
  target: Peer,
  source: Peer,
): Array<{ plain: string; technical: string }> {
  const items: Array<{ plain: string; technical: string }> = []

  const sourceCounter = replicaFor(source, source.gcounter.length)
  const targetCounter = replicaFor(target, target.gcounter.length)
  for (let replica = 0; replica < source.gcounter.length; replica += 1) {
    if (sourceCounter.versionFor(BigInt(replica)) > targetCounter.versionFor(BigInt(replica))) {
      items.push({ plain: `headcount from Camp ${replica}`, technical: `G(log ${replica})` })
    }
  }

  if (missingOrSetRecords(target, source, source.gcounter.length)) {
    items.push({ plain: 'supply records', technical: 'OR(log)' })
  }

  return items
}

function orsetReplicaFor(peer: Pick<Peer, 'id' | 'orset'>): SafeMeshStringOrSetReplica {
  const replica = new SafeMeshStringOrSetReplica(BigInt(peer.id))
  try {
    if (peer.orset.length > 0) replica.mergeLogBytes(peer.orset)
    return replica
  } catch (error) {
    replica.free()
    throw error
  }
}

export function readORSet(log: Uint8Array): string[] {
  const replica = orsetReplicaFor({ id: 0, orset: log })
  try {
    return replica.elements()
  } finally {
    replica.free()
  }
}

// Compare the core's ordered raw snapshots, without deriving membership or merging.
function orsetStateKey(peer: Peer): string {
  const replica = orsetReplicaFor(peer)
  try {
    const entries = replica.addEntries().map((entry) => {
      try {
        return [entry.element(), entry.token().toString()]
      } finally {
        entry.free()
      }
    })
    return JSON.stringify([entries, Array.from(replica.tombstones(), String)])
  } finally {
    replica.free()
  }
}

function missingOrSetRecords(target: Peer, source: Peer, peerCount: number): boolean {
  const sourceSet = orsetReplicaFor(source)
  const targetSet = orsetReplicaFor(target)
  try {
    return Array.from({ length: peerCount }, (_, id) =>
      sourceSet.versionFor(BigInt(id)) > targetSet.versionFor(BigInt(id)),
    ).some(Boolean)
  } finally {
    sourceSet.free()
    targetSet.free()
  }
}

function normalizePeer(peer: Peer): Peer {
  return { ...peer, localTally: Math.max(peer.localTally, peer.gcounter[peer.id] ?? 0) }
}

function emptyGCounterPeer(id: number, replicas: number): Pick<Peer, 'gcounter' | 'gcounterLog'> {
  return snapshotGCounterPeer({ id, gcounter: [], gcounterLog: new Uint8Array(), orset: new Uint8Array(), localTally: 0 }, new SafeMeshGCounterReplica(BigInt(id), replicas))
}

function replicaFor(peer: Peer, replicas: number): SafeMeshGCounterReplica {
  const replica = new SafeMeshGCounterReplica(BigInt(peer.id), replicas)
  if (peer.gcounterLog.length > 0) replica.mergeLogBytes(peer.gcounterLog)
  return replica
}

function snapshotGCounterPeer(peer: Peer, counter: SafeMeshGCounterReplica): Peer {
  return {
    ...peer,
    gcounter: Array.from(counter.state(), Number),
    gcounterLog: counter.logBytes(),
    localTally: Math.max(peer.localTally, Number(counter.state()[peer.id] ?? 0n)),
  }
}

function appendLog(
  sim: Simulation,
  plain: string,
  tone: LogEntry['tone'],
  technical = plain,
): Simulation {
  const entry = { id: `l${sim.nextId}-${sim.log.length}`, at: sim.now, plain, technical, tone }
  return { ...sim, log: [entry, ...sim.log].slice(0, 80) }
}

function describeDelta(delta: MeshDelta): string {
  if (delta.kind === 'gcounter.bump') return `G(${delta.replica}:=${delta.tally})`
  if (delta.kind === 'orset.add') return `OR-add(${delta.element}, ${delta.token})`
  if (delta.kind === 'orset.log') return 'OR(log)'
  return `OR-remove(${delta.tokens.length} tokens)`
}

function plainDeliver(packet: Packet): string {
  if (packet.delta.kind === 'gcounter.bump') {
    return `Camp ${packet.to} heard Camp ${packet.delta.replica}'s headcount`
  }
  if (packet.delta.kind === 'orset.add') {
    return `Camp ${packet.to} received '${packet.delta.element}' from Camp ${packet.from}`
  }
  if (packet.delta.kind === 'orset.log') return `Camp ${packet.to} received supply records from Camp ${packet.from}`
  return `Camp ${packet.to} received removal notes from Camp ${packet.from}`
}

function plainRepair(packet: Packet): string {
  if (packet.delta.kind === 'gcounter.bump') {
    return `Camp ${packet.to} recovered Camp ${packet.delta.replica}'s headcount from Camp ${packet.from}`
  }
  if (packet.delta.kind === 'orset.add') {
    return `Camp ${packet.to} recovered '${packet.delta.element}' from Camp ${packet.from}`
  }
  if (packet.delta.kind === 'orset.log') return `Camp ${packet.to} recovered supply records from Camp ${packet.from}`
  return `Camp ${packet.to} recovered removal notes from Camp ${packet.from}`
}

function plainDrop(packet: Packet): string {
  if (packet.delta.kind === 'gcounter.bump') {
    return `Camp ${packet.to} missed Camp ${packet.delta.replica}'s headcount (signal dropped)`
  }
  if (packet.delta.kind === 'orset.add') {
    return `Camp ${packet.to} missed '${packet.delta.element}' (signal dropped)`
  }
  if (packet.delta.kind === 'orset.log') return `Camp ${packet.to} missed supply records (signal dropped)`
  return `Camp ${packet.to} missed removal notes (signal dropped)`
}
