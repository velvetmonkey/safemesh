import {
  addDelta,
  applyGCounterDelta,
  applyORSetDelta,
  bottomGCounter,
  bottomORSet,
  bumpDelta,
  mergeGCounter,
  mergeORSet,
  observedTokens,
  readGCounter,
  readORSet,
  sameGCounter,
  sameORSet,
  type GCounterDelta,
  type GCounterState,
  type ORSetDelta,
  type ORSetState,
} from '../crdt'

export type MeshDelta = GCounterDelta | ORSetDelta

export type Peer = {
  id: number
  gcounter: GCounterState
  orset: ORSetState
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
      gcounter: bottomGCounter(peerCount),
      orset: bottomORSet(),
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
  return emitDelta(
    sim,
    peerId,
    bumpDelta(peerId, tally),
    `Camp ${peerId} increases headcount to ${tally}`,
    `peer ${peerId} bumps G-Counter to ${tally}`,
  )
}

export function addElement(sim: Simulation, peerId: number, element: string): Simulation {
  const peer = sim.peers[peerId]
  const clean = element.trim()
  if (!peer || clean.length === 0) return sim
  const token = `p${peerId}-${sim.nextId}`
  return emitDelta(
    sim,
    peerId,
    addDelta(clean, token),
    `Camp ${peerId} added '${clean}' to the supply list`,
    `peer ${peerId} adds "${clean}" (${token})`,
  )
}

export function removeElement(sim: Simulation, peerId: number, element: string): Simulation {
  const peer = sim.peers[peerId]
  if (!peer) return sim
  const tokens = observedTokens(peer.orset, element)
  if (tokens.length === 0) {
    return appendLog(
      sim,
      `Camp ${peerId} cannot remove '${element}' because it has not seen it`,
      'drop',
      `peer ${peerId} remove skipped; "${element}" is not observed`,
    )
  }
  return emitDelta(
    sim,
    peerId,
    { kind: 'orset.remove', tokens },
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
  gcounterValue: number
  orsetElements: string[]
} {
  const [first] = sim.peers
  if (!first) {
    return { converged: true, sameRawState: true, sameReads: true, gcounterValue: 0, orsetElements: [] }
  }

  const sameRawState = sim.peers.every(
    (peer) => sameGCounter(peer.gcounter, first.gcounter) && sameORSet(peer.orset, first.orset),
  )
  const firstGRead = readGCounter(first.gcounter)
  const firstORead = readORSet(first.orset).join('\u0000')
  const sameReads = sim.peers.every(
    (peer) => readGCounter(peer.gcounter) === firstGRead && readORSet(peer.orset).join('\u0000') === firstORead,
  )

  return {
    converged: sameRawState && sim.queue.length === 0,
    sameRawState,
    sameReads,
    gcounterValue: firstGRead,
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
  if (delta.kind === 'gcounter.bump') {
    return `10 ${hex(delta.replica)} ${hex(delta.tally)}`
  }
  if (delta.kind === 'orset.add') {
    return `20 ${asciiHex(delta.element)} ${asciiHex(delta.token)}`
  }
  return `21 ${delta.tokens.map(asciiHex).join(' ')}`
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
    const gcounter = applyGCounterDelta(peer.gcounter, delta)
    return {
      ...peer,
      gcounter,
      localTally: peer.id === delta.replica ? Math.max(peer.localTally, delta.tally) : peer.localTally,
    }
  }
  return { ...peer, orset: applyORSetDelta(peer.orset, delta) }
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

      const merged = {
        gcounter: mergeGCounter(left.gcounter, right.gcounter),
        orset: mergeORSet(left.orset, right.orset),
      }
      peers = peers.map((peer) => {
        if (peer.id === left.id) return normalizePeer({ ...left, ...merged })
        if (peer.id === right.id) return normalizePeer({ ...right, ...merged })
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
  const union = sim.peers.reduce(
    (merged, peer) => ({
      gcounter: mergeGCounter(merged.gcounter, peer.gcounter),
      orset: mergeORSet(merged.orset, peer.orset),
    }),
    { gcounter: bottomGCounter(sim.peers.length), orset: bottomORSet() },
  )

  for (const target of sim.peers) {
    union.gcounter.forEach((value, replica) => {
      if (value <= (target.gcounter[replica] ?? 0)) return
      const source = sim.peers.find((peer) => peer.id !== target.id && (peer.gcounter[replica] ?? 0) >= value)
      if (!source) return
      packets.push({
        id: `r${nextId}-${repairIndex}`,
        from: source.id,
        to: target.id,
        delta: bumpDelta(replica, value),
        sentAt: sim.now,
        deliverAt: sim.now + sim.latencyMs + 160 + repairIndex * 210,
        phase: 'repair',
      })
      nextId += 1
      repairIndex += 1
    })

    for (const [token, element] of Object.entries(union.orset.adds)) {
      if (target.orset.adds[token]) continue
      const source = sim.peers.find((peer) => peer.id !== target.id && peer.orset.adds[token] === element)
      if (!source) continue
      packets.push({
        id: `r${nextId}-${repairIndex}`,
        from: source.id,
        to: target.id,
        delta: addDelta(element, token),
        sentAt: sim.now,
        deliverAt: sim.now + sim.latencyMs + 160 + repairIndex * 210,
        phase: 'repair',
      })
      nextId += 1
      repairIndex += 1
    }

    for (const token of Object.keys(union.orset.tombstones)) {
      if (target.orset.tombstones[token]) continue
      const source = sim.peers.find((peer) => peer.id !== target.id && peer.orset.tombstones[token])
      if (!source) continue
      packets.push({
        id: `r${nextId}-${repairIndex}`,
        from: source.id,
        to: target.id,
        delta: { kind: 'orset.remove', tokens: [token] },
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

function hex(value: number): string {
  return value.toString(16).padStart(2, '0').slice(-4).toUpperCase()
}

function asciiHex(value: string): string {
  return [...value]
    .slice(0, 10)
    .map((char) => char.charCodeAt(0).toString(16).padStart(2, '0').toUpperCase())
    .join('')
}

function backfillDescriptions(
  target: Peer,
  source: Peer,
): Array<{ plain: string; technical: string }> {
  const items: Array<{ plain: string; technical: string }> = []

  source.gcounter.forEach((value, replica) => {
    if (value > (target.gcounter[replica] ?? 0)) {
      items.push({ plain: `headcount from Camp ${replica}`, technical: `G(${replica}:=${value})` })
    }
  })

  for (const [token, element] of Object.entries(source.orset.adds)) {
    if (!target.orset.adds[token]) {
      items.push({ plain: `'${element}'`, technical: `OR-add(${element}, ${token})` })
    }
  }

  for (const token of Object.keys(source.orset.tombstones)) {
    if (!target.orset.tombstones[token]) {
      items.push({ plain: `a removal marker (${token})`, technical: `OR-remove-token(${token})` })
    }
  }

  return items
}

function normalizePeer(peer: Peer): Peer {
  return { ...peer, localTally: Math.max(peer.localTally, peer.gcounter[peer.id] ?? 0) }
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
  return `OR-remove(${delta.tokens.length} tokens)`
}

function plainDeliver(packet: Packet): string {
  if (packet.delta.kind === 'gcounter.bump') {
    return `Camp ${packet.to} heard Camp ${packet.delta.replica}'s headcount`
  }
  if (packet.delta.kind === 'orset.add') {
    return `Camp ${packet.to} received '${packet.delta.element}' from Camp ${packet.from}`
  }
  return `Camp ${packet.to} received removal notes from Camp ${packet.from}`
}

function plainRepair(packet: Packet): string {
  if (packet.delta.kind === 'gcounter.bump') {
    return `Camp ${packet.to} recovered Camp ${packet.delta.replica}'s headcount from Camp ${packet.from}`
  }
  if (packet.delta.kind === 'orset.add') {
    return `Camp ${packet.to} recovered '${packet.delta.element}' from Camp ${packet.from}`
  }
  return `Camp ${packet.to} recovered removal notes from Camp ${packet.from}`
}

function plainDrop(packet: Packet): string {
  if (packet.delta.kind === 'gcounter.bump') {
    return `Camp ${packet.to} missed Camp ${packet.delta.replica}'s headcount (signal dropped)`
  }
  if (packet.delta.kind === 'orset.add') {
    return `Camp ${packet.to} missed '${packet.delta.element}' (signal dropped)`
  }
  return `Camp ${packet.to} missed removal notes (signal dropped)`
}
