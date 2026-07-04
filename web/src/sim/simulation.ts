import {
  addDelta,
  applyGCounterDelta,
  applyORSetDelta,
  bottomGCounter,
  bottomORSet,
  bumpDelta,
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
  deliverAt: number
}

export type LogEntry = {
  id: string
  at: number
  text: string
  tone: 'send' | 'deliver' | 'drop' | 'partition' | 'merge'
}

export type Simulation = {
  peers: Peer[]
  queue: Packet[]
  partitioned: boolean
  latencyMs: number
  dropRate: number
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
    log: appendLog(next, `reset mesh to ${peerCount} peers`, 'partition').log,
  }
}

export function setPartitioned(sim: Simulation, partitioned: boolean): Simulation {
  return appendLog(
    { ...sim, partitioned },
    partitioned ? 'partition enabled: generated deltas stay local' : 'partition healed: queued gossip resumes',
    'partition',
  )
}

export function setLatency(sim: Simulation, latencyMs: number): Simulation {
  return { ...sim, latencyMs }
}

export function setDropRate(sim: Simulation, dropRate: number): Simulation {
  return { ...sim, dropRate }
}

export function bumpCounter(sim: Simulation, peerId: number): Simulation {
  const peer = sim.peers[peerId]
  if (!peer) return sim
  const tally = peer.localTally + 1
  return emitDelta(sim, peerId, bumpDelta(peerId, tally), `peer ${peerId} bumps G-Counter to ${tally}`)
}

export function addElement(sim: Simulation, peerId: number, element: string): Simulation {
  const peer = sim.peers[peerId]
  const clean = element.trim()
  if (!peer || clean.length === 0) return sim
  const token = `p${peerId}-${sim.nextId}`
  return emitDelta(sim, peerId, addDelta(clean, token), `peer ${peerId} adds "${clean}" (${token})`)
}

export function removeElement(sim: Simulation, peerId: number, element: string): Simulation {
  const peer = sim.peers[peerId]
  if (!peer) return sim
  const tokens = observedTokens(peer.orset, element)
  if (tokens.length === 0) {
    return appendLog(sim, `peer ${peerId} remove skipped; "${element}" is not observed`, 'drop')
  }
  return emitDelta(
    sim,
    peerId,
    { kind: 'orset.remove', tokens },
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
        queue: [...next.queue, { ...packet, deliverAt: now + sim.latencyMs }],
      }
      continue
    }

    if (random() < sim.dropRate) {
      next = appendLog(next, `dropped ${describeDelta(packet.delta)} ${packet.from}->${packet.to}`, 'drop')
      continue
    }

    next = deliverPacket(next, packet)
  }

  return next
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

function emitDelta(sim: Simulation, peerId: number, delta: MeshDelta, message: string): Simulation {
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
      deliverAt: sim.now + sim.latencyMs + index * 90,
    }))

  return appendLog(
    {
      ...sim,
      peers,
      queue: [...sim.queue, ...packets],
      nextId: sim.nextId + 1,
    },
    sim.partitioned ? `${message}; held by partition` : `${message}; queued to ${packets.length} peers`,
    'send',
  )
}

function deliverPacket(sim: Simulation, packet: Packet): Simulation {
  const peers = sim.peers.map((peer) => (peer.id === packet.to ? applyDeltaToPeer(peer, packet.delta) : peer))
  return appendLog(
    { ...sim, peers },
    `delivered ${describeDelta(packet.delta)} ${packet.from}->${packet.to}`,
    'deliver',
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

function appendLog(sim: Simulation, text: string, tone: LogEntry['tone']): Simulation {
  const entry = { id: `l${sim.nextId}-${sim.log.length}`, at: sim.now, text, tone }
  return { ...sim, log: [entry, ...sim.log].slice(0, 80) }
}

function describeDelta(delta: MeshDelta): string {
  if (delta.kind === 'gcounter.bump') return `G(${delta.replica}:=${delta.tally})`
  if (delta.kind === 'orset.add') return `OR-add(${delta.element}, ${delta.token})`
  return `OR-remove(${delta.tokens.length} tokens)`
}
