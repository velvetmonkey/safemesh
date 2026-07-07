import {
  addElement,
  bumpCounter,
  createSimulation,
  dropCounterPacketToPeer,
  duplicateNextPacket,
  queueAntiEntropyPackets,
  reorderQueue,
  setAntiEntropyMs,
  setDropRate,
  setLatency,
  setPartitioned,
  tick,
  type Simulation,
} from './simulation'

export type GuideTone = 'ok' | 'warn' | 'bad'

export type GuideStep = {
  id: string
  title: string
  plainExplanation: string
  watchFor: string
  stageCue?: string
  motionMs: number
  tone: GuideTone
  action: (sim: Simulation) => Simulation
}

export type GuideScenario = {
  id: string
  label: string
  headline: string
  setup: string
  whyItMatters: string
  successState: string
  steps: GuideStep[]
}

export const GUIDE_SCENARIOS: GuideScenario[] = [
  {
    id: 'normal',
    label: 'Normal delivery',
    headline: 'A message takes the easy route.',
    setup: 'Replica 0 records the vaccine handoff and an audit count. The network behaves, so the message reaches every other replica.',
    whyItMatters: 'This is the baseline: local state changes first, then the same modeled record set reaches the rest of the mesh.',
    successState: 'All replicas show count 1 and vaccine.',
    steps: [
      {
        id: 'start',
        title: 'Start with quiet replicas.',
        plainExplanation: 'Nothing has happened yet. Every replica has the same empty modeled state.',
        watchFor: 'All four replicas show count 0 and no records.',
        stageCue: 'Baseline: all four replicas are empty.',
        motionMs: 0,
        tone: 'ok',
        action: (sim) => sim,
      },
      {
        id: 'send',
        title: 'Replica 0 records the handoff.',
        plainExplanation: 'One local write creates small deltas for vaccine custody and audit count 1.',
        watchFor: 'Replica 0 changes first, while packet chips leave for the other replicas.',
        stageCue: 'Local write first: Replica 0 changes before anyone else hears it.',
        motionMs: 360,
        tone: 'bad',
        action: recordVaccineAndAudit,
      },
      {
        id: 'deliver',
        title: 'The messages arrive.',
        plainExplanation: 'The queued deltas reach the other replicas. Each merge adds the same modeled facts.',
        watchFor: 'The remaining replicas catch up to vaccine and count 1.',
        stageCue: 'Normal order: packet chips are numbered by the order they will arrive.',
        motionMs: 1800,
        tone: 'ok',
        action: (sim) => sim,
      },
    ],
  },
  {
    id: 'duplicate',
    label: 'Duplicate replay',
    headline: 'The same message arrives twice.',
    setup: 'Replica 0 records the same vaccine handoff. The transport accidentally duplicates one in-flight delta.',
    whyItMatters: 'Real networks retry. The visible lesson is that replay can be noisy without changing the modeled value.',
    successState: 'All replicas still show exactly count 1 and vaccine.',
    steps: [
      {
        id: 'start',
        title: 'Start with the same quiet mesh.',
        plainExplanation: 'Nothing has happened yet. This scenario will send one vaccine handoff, then replay one message.',
        watchFor: 'All replicas still agree before the duplicate appears.',
        stageCue: 'Baseline: the replay has not happened yet.',
        motionMs: 0,
        tone: 'ok',
        action: (sim) => sim,
      },
      {
        id: 'send',
        title: 'Send the handoff once.',
        plainExplanation: 'Replica 0 records vaccine custody and audit count 1, then sends those deltas out.',
        watchFor: 'Several packet chips leave Replica 0.',
        stageCue: 'The original packets leave first.',
        motionMs: 220,
        tone: 'bad',
        action: recordVaccineAndAudit,
      },
      {
        id: 'duplicate',
        title: 'Replay one delta.',
        plainExplanation: 'The transport duplicates one queued message. The duplicate is visible, but merge is idempotent.',
        watchFor: 'A dashed packet appears. It should not create a second vaccine or bump count past 1.',
        stageCue: 'The dashed chip is the replay. It can arrive twice without counting twice.',
        motionMs: 520,
        tone: 'bad',
        action: duplicateNextPacket,
      },
      {
        id: 'settle',
        title: 'Let every delivery land.',
        plainExplanation: 'Originals and duplicates finish delivery. Duplicate replay becomes harmless noise.',
        watchFor: 'Every replica ends at the same single vaccine record and count 1.',
        stageCue: 'Replay is noise: the final read is still one vaccine and count 1.',
        motionMs: 1800,
        tone: 'ok',
        action: (sim) => sim,
      },
    ],
  },
  {
    id: 'reorder',
    label: 'Out-of-order',
    headline: 'Messages arrive in the wrong order.',
    setup: 'Replica 0 sends vaccine custody and audit count 1. The queue is then reversed before delivery.',
    whyItMatters: 'Builders should see that the demo is not depending on a polite FIFO transport.',
    successState: 'All replicas show count 1 and vaccine after the reordered delivery completes.',
    steps: [
      {
        id: 'start',
        title: 'Start with the same quiet mesh.',
        plainExplanation: 'Nothing has happened yet. This scenario will send the same vaccine handoff, then change arrival order.',
        watchFor: 'All replicas begin from the empty shared state.',
        stageCue: 'Baseline: no order has been changed yet.',
        motionMs: 0,
        tone: 'ok',
        action: (sim) => sim,
      },
      {
        id: 'send',
        title: 'Create two facts to deliver.',
        plainExplanation: 'Replica 0 records the vaccine and audit count. Those facts are now in flight.',
        watchFor: 'Multiple packets queue up from the same source replica.',
        stageCue: 'Normal queue is visible first: ADD and G0 chips are waiting to deliver.',
        motionMs: 220,
        tone: 'bad',
        action: recordVaccineAndAudit,
      },
      {
        id: 'reorder',
        title: 'Reverse the queue.',
        plainExplanation: 'The transport changes the arrival order. The route is messy; the modeled merge remains stable.',
        watchFor: 'Packet labels switch to REV. The first reversed chip is now the next one to land.',
        stageCue: 'Reversed order: REV 1 arrives first, even though it was not first in the original queue.',
        motionMs: 900,
        tone: 'bad',
        action: reorderQueue,
      },
      {
        id: 'settle',
        title: 'Deliver the reordered messages.',
        plainExplanation: 'The messages land in the new order and the replicas still reach the same read.',
        watchFor: 'Every replica ends at count 1 with vaccine.',
        stageCue: 'Wrong order is okay here: each CRDT merge still reaches the same read.',
        motionMs: 1800,
        tone: 'ok',
        action: (sim) => sim,
      },
    ],
  },
  {
    id: 'drop',
    label: 'Dropped message',
    headline: 'One replica misses a message, then catches up.',
    setup: 'Replica 0 records the vaccine handoff. The transport drops one delivery before anti-entropy repairs the gap.',
    whyItMatters: 'The important idea is not magic delivery. It is that missing modeled state can be backfilled later.',
    successState: 'All replicas show count 1 and vaccine after anti-entropy recovery.',
    steps: [
      {
        id: 'start',
        title: 'Start with the same quiet mesh.',
        plainExplanation: 'Nothing has happened yet. This scenario will lose one delivery, then recover the missing fact.',
        watchFor: 'All replicas begin from the empty shared state.',
        stageCue: 'Baseline: watch Replica 3 during this scenario.',
        motionMs: 0,
        tone: 'ok',
        action: (sim) => sim,
      },
      {
        id: 'send',
        title: 'Send the handoff.',
        plainExplanation: 'Replica 0 records vaccine custody and audit count 1, then sends those facts out.',
        watchFor: 'Replica 0 is ahead while packets are still moving.',
        stageCue: 'Replica 0 records the fact. Replica 3 has not heard it yet.',
        motionMs: 220,
        tone: 'bad',
        action: recordVaccineAndAudit,
      },
      {
        id: 'drop',
        title: 'Drop Replica 3\'s count.',
        plainExplanation: 'One counter delivery to Replica 3 is lost. Other messages can still arrive, so the gap stays visible instead of hidden.',
        watchFor: 'Replica 3 remains at count 0 while the others move toward count 1.',
        stageCue: 'Dropped: Replica 3 missed G0. It should stay at 0 until a repair chip reaches it.',
        motionMs: 1500,
        tone: 'bad',
        action: (sim) => dropCounterPacketToPeer(sim, 3),
      },
      {
        id: 'recover',
        title: 'Backfill the missing fact.',
        plainExplanation: 'Anti-entropy compares connected replicas, queues a visible repair message, and only then fills the missing modeled record.',
        watchFor: 'A SYNC G0 chip travels to Replica 3 before its count changes to 1.',
        stageCue: 'Backfill path: SYNC G0 crosses to Replica 3. No state changes until it lands.',
        motionMs: 1900,
        tone: 'ok',
        action: queueAntiEntropyPackets,
      },
    ],
  },
  {
    id: 'partition',
    label: 'Partition heal',
    headline: 'The mesh splits, then heals.',
    setup: 'Replica 0 records the same vaccine handoff while the link is cut. The isolated mesh later reconnects and reconciles.',
    whyItMatters: 'This is the offline-first story: work can continue during a split, and modeled state converges after records meet.',
    successState: 'All replicas show count 1 and vaccine after the heal.',
    steps: [
      {
        id: 'start',
        title: 'Start with the same quiet mesh.',
        plainExplanation: 'Nothing has happened yet. This scenario will cut the route before the handoff finishes.',
        watchFor: 'All replicas begin from the empty shared state.',
        stageCue: 'Baseline: the link is still up.',
        motionMs: 0,
        tone: 'ok',
        action: (sim) => sim,
      },
      {
        id: 'send',
        title: 'Create the handoff.',
        plainExplanation: 'Replica 0 records vaccine custody and audit count 1. The messages start to leave.',
        watchFor: 'Replica 0 changes first while packets enter the radio layer.',
        stageCue: 'The handoff starts crossing the mesh.',
        motionMs: 220,
        tone: 'bad',
        action: recordVaccineAndAudit,
      },
      {
        id: 'cut',
        title: 'Cut the link before delivery finishes.',
        plainExplanation: 'The partition blocks the route. Some replicas have not seen the handoff yet.',
        watchFor: 'A partition wall appears and the mesh remains visibly split.',
        stageCue: 'Partition wall is up: queued packets cannot cross yet.',
        motionMs: 900,
        tone: 'warn',
        action: (sim) => setPartitioned(sim, true),
      },
      {
        id: 'heal',
        title: 'Reconnect and reconcile.',
        plainExplanation: 'The wall comes down first. The already-queued messages then cross the link during this step, so the repair is visible.',
        watchFor: 'The partition wall disappears, packets move across, and only then do the isolated replicas catch up.',
        stageCue: 'Heal path: the wall is down, but the mesh is not fixed until the queued chips land.',
        motionMs: 1900,
        tone: 'ok',
        action: (sim) => setAntiEntropyMs(setPartitioned(sim, false), 7000),
      },
    ],
  },
]

export function createScenarioBaseline(): Simulation {
  return setLatency(setDropRate(setAntiEntropyMs(createSimulation(4), 7000), 0), 720)
}

export function buildScenarioSimulation(
  scenarioId: string,
  stepIndex: number,
  elapsedMs = currentScenarioStep(scenarioId, stepIndex).motionMs,
): Simulation {
  const scenario = currentScenario(scenarioId)
  const clampedIndex = clampScenarioStep(scenario.id, stepIndex)
  let sim = createScenarioBaseline()

  for (let index = 0; index <= clampedIndex; index += 1) {
    const step = scenario.steps[index]
    sim = step.action(sim)
    const settleMs = index === clampedIndex ? clamp(elapsedMs, 0, step.motionMs) : step.motionMs
    if (settleMs > 0) sim = advanceScenarioSimulation(sim, settleMs)
  }

  return sim
}

export function advanceScenarioSimulation(sim: Simulation, elapsedMs: number): Simulation {
  return tick(sim, elapsedMs, () => 1)
}

export function currentScenario(scenarioId: string): GuideScenario {
  return GUIDE_SCENARIOS.find((scenario) => scenario.id === scenarioId) ?? GUIDE_SCENARIOS[0]
}

export function currentScenarioStep(scenarioId: string, stepIndex: number): GuideStep {
  const scenario = currentScenario(scenarioId)
  return scenario.steps[clampScenarioStep(scenario.id, stepIndex)]
}

export function clampScenarioStep(scenarioId: string, stepIndex: number): number {
  const scenario = currentScenario(scenarioId)
  return Math.min(scenario.steps.length - 1, Math.max(0, Math.round(stepIndex)))
}

function recordVaccineAndAudit(sim: Simulation): Simulation {
  return bumpCounter(addElement(sim, 0, 'vaccine'), 0)
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value))
}
