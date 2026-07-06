import {
  addElement,
  bumpCounter,
  createSimulation,
  dropNextPacket,
  duplicateNextPacket,
  reorderQueue,
  runAntiEntropyNow,
  setAntiEntropyMs,
  setDropRate,
  setLatency,
  setPartitioned,
  tick,
  type Simulation,
} from './simulation'

export type GuideSceneTone = 'ok' | 'warn' | 'bad'

export type GuideScene = {
  id: string
  title: string
  narrative: string
  watchFor: string
  motionMs: number
  expectedTone: GuideSceneTone
  apply: (sim: Simulation) => Simulation
}

export const GUIDE_SCENES: GuideScene[] = [
  {
    id: 'baseline',
    title: 'Start with four quiet replicas.',
    narrative: 'Every camp begins with the same empty modeled state. Nothing has crossed the radio layer yet.',
    watchFor: 'All replicas show count 0 and an empty set.',
    motionMs: 0,
    expectedTone: 'ok',
    apply: (sim) => sim,
  },
  {
    id: 'vaccine',
    title: 'Replica 0 records vaccine custody.',
    narrative: 'One local append creates compact deltas. The source replica changes immediately; the other replicas wait for delivery.',
    watchFor: 'Packet chips leave Replica 0 before the rest of the mesh catches up.',
    motionMs: 260,
    expectedTone: 'bad',
    apply: (sim) => addElement(sim, 0, 'vaccine'),
  },
  {
    id: 'duplicate',
    title: 'Duplicate an in-flight delta.',
    narrative: 'The same record can arrive more than once. Idempotent merge makes replay visible but harmless to the modeled value.',
    watchFor: 'One packet gets a dashed duplicate while the set still reads as one vaccine record.',
    motionMs: 120,
    expectedTone: 'bad',
    apply: duplicateNextPacket,
  },
  {
    id: 'reorder',
    title: 'Reverse delivery order.',
    narrative: 'The transport takes a messier route. The proof-backed CRDT semantics are about eventual merge, not a polite network.',
    watchFor: 'Queued packets swap order; the state can still settle once records arrive.',
    motionMs: 120,
    expectedTone: 'bad',
    apply: reorderQueue,
  },
  {
    id: 'drop',
    title: 'Drop one radio packet.',
    narrative: 'One replica misses a record. The demo leaves that gap visible instead of pretending the network behaved.',
    watchFor: 'The log calls out the miss, and at least one replica remains behind.',
    motionMs: 700,
    expectedTone: 'bad',
    apply: dropNextPacket,
  },
  {
    id: 'partition',
    title: 'Cut the mesh into two sides.',
    narrative: 'A partition is a modeled transport fault. Both sides can keep accepting local records while isolated.',
    watchFor: 'The wall appears through the field and cross-partition links turn into fault lines.',
    motionMs: 0,
    expectedTone: 'warn',
    apply: (sim) => setPartitioned(sim, true),
  },
  {
    id: 'freezer',
    title: 'One side adds freezer custody.',
    narrative: 'Replica 1 keeps working while the link is down. That local fact cannot reach the other side yet.',
    watchFor: 'Freezer appears on one side first, with outgoing records stuck behind the partition.',
    motionMs: 520,
    expectedTone: 'warn',
    apply: (sim) => addElement(sim, 1, 'freezer'),
  },
  {
    id: 'audit',
    title: 'The other side increments the audit count.',
    narrative: 'Replica 2 records a separate counter event. Now the split is easy to see: each side knows something different.',
    watchFor: 'The central count and replica cards disagree until the missing records meet.',
    motionMs: 520,
    expectedTone: 'warn',
    apply: (sim) => bumpCounter(sim, 2),
  },
  {
    id: 'heal',
    title: 'Heal the link and run anti-entropy.',
    narrative: 'When the link comes back, anti-entropy exchanges missing state between connected replicas.',
    watchFor: 'Recovery entries appear in the log while the field pulses toward a shared state.',
    motionMs: 900,
    expectedTone: 'bad',
    apply: (sim) => runAntiEntropyNow(setPartitioned(sim, false)),
  },
  {
    id: 'inspect',
    title: 'Inspect the converged modeled state.',
    narrative: 'The messy route changed timing, not the final modeled value once the record sets met.',
    watchFor: 'Every replica agrees on vaccine, freezer, and audit count 1 with no queued packets left.',
    motionMs: 2400,
    expectedTone: 'ok',
    apply: (sim) => sim,
  },
]

export function createGuidedBaseline(): Simulation {
  return setLatency(setDropRate(setAntiEntropyMs(createSimulation(4), 7000), 0), 720)
}

export function buildGuidedSimulation(sceneIndex: number, elapsedMs = currentScene(sceneIndex).motionMs): Simulation {
  const clampedIndex = clampGuideIndex(sceneIndex)
  let sim = createGuidedBaseline()

  for (let index = 0; index <= clampedIndex; index += 1) {
    const scene = GUIDE_SCENES[index]
    sim = scene.apply(sim)
    const settleMs = index === clampedIndex ? clamp(elapsedMs, 0, scene.motionMs) : scene.motionMs
    if (settleMs > 0) sim = advanceGuideSimulation(sim, settleMs)
  }

  return sim
}

export function advanceGuideSimulation(sim: Simulation, elapsedMs: number): Simulation {
  return tick(sim, elapsedMs, () => 1)
}

export function currentScene(sceneIndex: number): GuideScene {
  return GUIDE_SCENES[clampGuideIndex(sceneIndex)]
}

export function clampGuideIndex(sceneIndex: number): number {
  return Math.min(GUIDE_SCENES.length - 1, Math.max(0, Math.round(sceneIndex)))
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value))
}
