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
  deliverAt: number
}

export type LogEntry = {
  id: string
  at: number
  plain: string
  technical: string
  tone: 'send' | 'deliver' | 'drop' | 'partition' | 'merge' | 'anti-entropy'
}

export type RecoveryPop = {
  id: string
  peerId: number
  label: string
  at: number
}

export type StormStats = {
  active: boolean
  longestDivergenceMs: number
  divergedSince: number | null
  alwaysRecovered: boolean
  sweeps: number
}

export type Simulation = {
  peers: Peer[]
  queue: Packet[]
  partitioned: boolean
  latencyMs: number
  dropRate: number
  antiEntropyMs: number
  nextAntiEntropyAt: number
  now: number
  nextId: number
  lastSweepAt: number | null
  lastConvergedAt: number | null
  lastProofAt: number | null
  recoveryPops: RecoveryPop[]
  storm: StormStats
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
    now: 0,
    nextId: 1,
    lastSweepAt: null,
    lastConvergedAt: null,
    lastProofAt: null,
    recoveryPops: [],
    storm: {
      active: false,
      longestDivergenceMs: 0,
      divergedSince: null,
      alwaysRecovered: true,
      sweeps: 0,
    },
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
    storm: sim.storm.active ? { ...next.storm, active: true, alwaysRecovered: sim.storm.alwaysRecovered } : next.storm,
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

export function startStorm(sim: Simulation): Simulation {
  return appendLog(
    {
      ...sim,
      partitioned: true,
      latencyMs: 1300,
      dropRate: 0.78,
      antiEntropyMs: 2000,
      nextAntiEntropyAt: sim.now + 2000,
      storm: { ...sim.storm, active: true, divergedSince: sim.storm.divergedSince ?? sim.now },
    },
    'Storm mode: network cut, heavy drops, long radio delay',
    'partition',
    'storm enabled: partition=true drop=0.78 latency=1300 antiEntropy=2000',
  )
}

export function stopStorm(sim: Simulation): Simulation {
  return appendLog(
    {
      ...sim,
      partitioned: false,
      dropRate: 0.12,
      latencyMs: 700,
      antiEntropyMs: 2000,
      nextAntiEntropyAt: sim.now,
      storm: { ...sim.storm, active: false },
    },
    'Storm easing: network back up, catch-up sweep ready',
    'partition',
    'storm disabled: partition=false drop=0.12 latency=700 antiEntropy=2000',
  )
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
        queue: [...next.queue, { ...packet, deliverAt: now + sim.latencyMs }],
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

  return updateStormStats(maybeRunAntiEntropy(next))
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

export function propagationStats(sim: Simulation): {
  missing: number
  total: number
  progress: number
  status: 'converged' | 'syncing' | 'split'
} {
  if (sim.partitioned) {
    const splitMissing = Math.max(1, countMissing(sim) + sim.queue.length)
    return { missing: splitMissing, total: Math.max(splitMissing, countPossible(sim)), progress: 0, status: 'split' }
  }

  const missing = countMissing(sim) + sim.queue.length
  const total = Math.max(missing, countPossible(sim), 1)
  return {
    missing,
    total,
    progress: missing === 0 ? 1 : Math.max(0, 1 - missing / total),
    status: missing === 0 && sim.queue.length === 0 ? 'converged' : 'syncing',
  }
}

export function runAntiEntropyNow(sim: Simulation): Simulation {
  return updateStormStats(runAntiEntropy({ ...sim, nextAntiEntropyAt: sim.now }))
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
      deliverAt: sim.now + sim.latencyMs + index * 90,
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
  return appendLog(
    { ...sim, peers },
    plainDeliver(packet),
    'deliver',
    `delivered ${describeDelta(packet.delta)} ${packet.from}->${packet.to}`,
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
  let next: Simulation = {
    ...sim,
    nextAntiEntropyAt: sim.now + sim.antiEntropyMs,
    lastSweepAt: sim.now,
    storm: { ...sim.storm, sweeps: sim.storm.sweeps + 1 },
  }
  const pops: RecoveryPop[] = []

  for (let i = 0; i < peers.length; i += 1) {
    for (let j = i + 1; j < peers.length; j += 1) {
      const left = peers[i]
      const right = peers[j]
      const rightToLeft = backfillDescriptions(left, right)
      const leftToRight = backfillDescriptions(right, left)

      if (rightToLeft.length === 0 && leftToRight.length === 0) continue

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
        pops.push({ id: `pop-${sim.now}-${left.id}-${pops.length}`, peerId: left.id, label: item.plain, at: sim.now })
        next = appendLog(
          { ...next, peers },
          `Camp ${left.id} recovered ${item.plain} from Camp ${right.id}`,
          'anti-entropy',
          `anti-entropy: peer ${left.id} backfilled ${item.technical} from peer ${right.id}`,
        )
      }
      for (const item of leftToRight) {
        pops.push({ id: `pop-${sim.now}-${right.id}-${pops.length}`, peerId: right.id, label: item.plain, at: sim.now })
        next = appendLog(
          { ...next, peers },
          `Camp ${right.id} recovered ${item.plain} from Camp ${left.id}`,
          'anti-entropy',
          `anti-entropy: peer ${right.id} backfilled ${item.technical} from peer ${left.id}`,
        )
      }
    }
  }

  const after = {
    ...next,
    peers,
    queue: next.queue.filter((packet) => {
      const target = peers[packet.to]
      return target ? !deltaCovered(target, packet.delta) : false
    }),
    recoveryPops: [...pops, ...sim.recoveryPops].slice(0, 30),
  }
  const status = convergence(after)
  if (status.converged && !convergence(sim).converged) {
    return {
      ...after,
      lastConvergedAt: sim.now,
      lastProofAt: sim.now,
      storm: { ...after.storm, divergedSince: null, alwaysRecovered: after.storm.alwaysRecovered },
    }
  }
  return after
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

function deltaCovered(peer: Peer, delta: MeshDelta): boolean {
  if (delta.kind === 'gcounter.bump') {
    return (peer.gcounter[delta.replica] ?? 0) >= delta.tally
  }
  if (delta.kind === 'orset.add') {
    return peer.orset.adds[delta.token] === delta.element
  }
  return delta.tokens.every((token) => peer.orset.tombstones[token])
}

function updateStormStats(sim: Simulation): Simulation {
  const status = convergence(sim)
  if (status.converged) {
    if (sim.storm.divergedSince === null) return sim
    const longest = Math.max(sim.storm.longestDivergenceMs, sim.now - sim.storm.divergedSince)
    return { ...sim, storm: { ...sim.storm, longestDivergenceMs: longest, divergedSince: null } }
  }

  if (sim.storm.divergedSince !== null) {
    return {
      ...sim,
      storm: {
        ...sim.storm,
        longestDivergenceMs: Math.max(sim.storm.longestDivergenceMs, sim.now - sim.storm.divergedSince),
      },
    }
  }
  return { ...sim, storm: { ...sim.storm, divergedSince: sim.now } }
}

function countPossible(sim: Simulation): number {
  const union = unionDigest(sim)
  return sim.peers.length * (union.gcounter.length + Object.keys(union.adds).length + Object.keys(union.tombstones).length)
}

function countMissing(sim: Simulation): number {
  const union = unionDigest(sim)
  let missing = 0
  for (const peer of sim.peers) {
    union.gcounter.forEach((value, index) => {
      if ((peer.gcounter[index] ?? 0) < value) missing += 1
    })
    for (const token of Object.keys(union.adds)) {
      if (!peer.orset.adds[token]) missing += 1
    }
    for (const token of Object.keys(union.tombstones)) {
      if (!peer.orset.tombstones[token]) missing += 1
    }
  }
  return missing
}

function unionDigest(sim: Simulation): {
  gcounter: GCounterState
  adds: ORSetState['adds']
  tombstones: ORSetState['tombstones']
} {
  return sim.peers.reduce(
    (acc, peer) => ({
      gcounter: mergeGCounter(acc.gcounter, peer.gcounter),
      adds: { ...acc.adds, ...peer.orset.adds },
      tombstones: { ...acc.tombstones, ...peer.orset.tombstones },
    }),
    { gcounter: bottomGCounter(sim.peers.length), adds: {}, tombstones: {} },
  )
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

function plainDrop(packet: Packet): string {
  if (packet.delta.kind === 'gcounter.bump') {
    return `Camp ${packet.to} missed Camp ${packet.delta.replica}'s headcount (signal dropped)`
  }
  if (packet.delta.kind === 'orset.add') {
    return `Camp ${packet.to} missed '${packet.delta.element}' (signal dropped)`
  }
  return `Camp ${packet.to} missed removal notes (signal dropped)`
}
