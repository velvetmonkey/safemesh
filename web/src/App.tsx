import { useEffect, useMemo, useState } from 'react'
import {
  BadgeCheck,
  CircleDot,
  Copy,
  Gauge,
  GitBranch,
  Pause,
  Play,
  Radio,
  RefreshCw,
  RotateCcw,
  Scissors,
  ShieldCheck,
  Shuffle,
  Wifi,
} from 'lucide-react'
import './App.css'
import { readGCounter, readORSet } from './crdt'
import {
  addElement,
  bumpCounter,
  convergence,
  createSimulation,
  dropNextPacket,
  duplicateNextPacket,
  removeElement,
  reorderQueue,
  runAntiEntropyNow,
  setAntiEntropyMs,
  setDropRate,
  setLatency,
  setPartitioned,
  setPeerCount,
  tick,
  wirePreview,
  type Packet,
  type Simulation,
} from './sim'

const SUPPLIES = ['vaccine', 'freezer', 'medkit', 'water', 'radio', 'insulin']

function App() {
  const [sim, setSim] = useState<Simulation>(() => createSimulation(4))
  const [selectedPeer, setSelectedPeer] = useState(0)
  const [element, setElement] = useState('vaccine')
  const [technical, setTechnical] = useState(false)
  const [calmMotion, setCalmMotion] = useState(() => window.matchMedia('(prefers-reduced-motion: reduce)').matches)
  const [tourCaption, setTourCaption] = useState('Fault the radio layer, then heal it. The replicas settle into one state.')
  const [tourRunning, setTourRunning] = useState(false)

  const status = useMemo(() => convergence(sim), [sim])
  const positions = useMemo(
    () => Array.from({ length: sim.peers.length }, (_, index) => peerPosition(index, sim.peers.length)),
    [sim.peers.length],
  )
  const selected = sim.peers[selectedPeer] ?? sim.peers[0]
  const recentAntiEntropy = sim.lastAntiEntropyAt !== null && sim.now - sim.lastAntiEntropyAt < 1500
  const story = storyState(sim, status)
  const StoryIcon = story.Icon

  useEffect(() => {
    const timer = window.setInterval(() => {
      setSim((current) => tick(current, calmMotion ? 500 : 180))
    }, calmMotion ? 500 : 180)
    return () => window.clearInterval(timer)
  }, [calmMotion])

  async function runGuidedTour() {
    if (tourRunning) return
    setTourRunning(true)
    setSelectedPeer(0)
    let scripted = setLatency(setDropRate(setAntiEntropyMs(createSimulation(4), 7000), 0.45), 650)

    const steps: Array<[string, (current: Simulation) => Simulation, number]> = [
      ['Replica 0 appends vaccine custody. Deltas leave as small records.', (s) => addElement(s, 0, 'vaccine'), 900],
      ['Duplicate one record. Idempotent merge means replay is noise, not a new event.', duplicateNextPacket, 800],
      ['Reverse the in-flight records. Order changes; the merge law does not.', reorderQueue, 850],
      ['Drop the next radio packet. A replica misses state for now.', dropNextPacket, 850],
      ['Partition the mesh. Both sides can keep appending while isolated.', (s) => setPartitioned(s, true), 800],
      ['Replica 1 adds freezer custody on one side of the split.', (s) => addElement(s, 1, 'freezer'), 850],
      ['Replica 2 increments the audit count on the other side.', (s) => bumpCounter(s, 2), 850],
      ['Heal the link. Anti-entropy pulls the missing record set across.', (s) => runAntiEntropyNow(setPartitioned(s, false)), 1200],
    ]

    for (const [caption, update, wait] of steps) {
      setTourCaption(caption)
      scripted = update(scripted)
      setSim(scripted)
      await sleep(calmMotion ? Math.max(250, wait / 2) : wait)
    }

    setTourCaption('Every replica holds the same modeled state. Delays and duplicates changed the route, not the result.')
    setTourRunning(false)
  }

  function healNow() {
    setSim((current) => runAntiEntropyNow(setPartitioned(current, false)))
  }

  return (
    <main className={`control-room ${calmMotion ? 'calm-motion' : ''}`}>
      <header className="topbar">
        <div>
          <p className="eyebrow">SafeMesh for builders</p>
          <h1>Watch replicas diverge, heal, and converge.</h1>
        </div>
        <div className={`state-pill ${story.tone}`} aria-live="polite">
          <StoryIcon aria-hidden="true" size={18} />
          <span>{story.label}</span>
        </div>
      </header>

      <section className="hero-grid" aria-label="Interactive convergence demo">
        <section className="stage" aria-label="Replica field">
          <svg className="link-layer" viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true">
            {linkPairs(sim.peers.length).map(([left, right]) => (
              <line
                key={`${left}-${right}`}
                className={sim.partitioned && crossesPartition(left, right) ? 'link partitioned' : 'link'}
                x1={positions[left].x}
                y1={positions[left].y}
                x2={positions[right].x}
                y2={positions[right].y}
              />
            ))}
            {sim.partitioned && <line className="partition-wall" x1="50" y1="10" x2="50" y2="90" />}
          </svg>

          {sim.queue.slice(0, 18).map((packet) => {
            const point = packetPoint(packet, sim, positions)
            return (
              <div
                key={packet.id}
                className={`packet ${packet.delta.kind.replace('.', '-')} ${packet.duplicated ? 'duplicated' : ''}`}
                style={{ left: `${point.x}%`, top: `${point.y}%` }}
                title={`demo wire sketch ${wirePreview(packet.delta)}`}
              >
                <span>{packetLabel(packet)}</span>
                <code>{wirePreview(packet.delta)}</code>
              </div>
            )
          })}

          <div className={`convergence-core ${status.converged ? 'ok' : sim.partitioned ? 'warn' : 'bad'} ${recentAntiEntropy ? 'pulse' : ''}`}>
            <span>{status.converged ? 'CONVERGED' : sim.partitioned ? 'PARTITIONED' : 'DIVERGED'}</span>
            <strong>{status.gcounterValue}</strong>
            <small>{status.orsetElements.join(' / ') || 'empty set'}</small>
          </div>

          {sim.peers.map((peer) => {
            const elements = readORSet(peer.orset)
            const position = positions[peer.id]
            return (
              <button
                type="button"
                key={peer.id}
                className={`replica ${peer.id === selectedPeer ? 'selected' : ''} ${sameRead(peer, status) ? 'aligned' : 'split'}`}
                style={{ left: `${position.x}%`, top: `${position.y}%` }}
                onClick={() => setSelectedPeer(peer.id)}
              >
                <span className="replica-kicker">Replica {peer.id}</span>
                <strong>{readGCounter(peer.gcounter)}</strong>
                <span>{elements.length > 0 ? elements.join(', ') : 'no records yet'}</span>
                <code>{digest(peer.gcounter, elements)}</code>
              </button>
            )
          })}
        </section>

        <aside className="side-rail" aria-label="Controls and claims">
          <div className="narrative">
            <p className="eyebrow">Narrative arc</p>
            <h2>{tourCaption}</h2>
            <p>{story.detail}</p>
          </div>

          <div className="control-bank" aria-label="Fault controls">
            <button type="button" onClick={runGuidedTour} disabled={tourRunning} title="Run the scripted break and heal tour">
              {tourRunning ? <Pause size={17} /> : <Play size={17} />}
              {tourRunning ? 'Running' : 'Guided run'}
            </button>
            <button type="button" onClick={() => setSim((current) => setPartitioned(current, !current.partitioned))} title="Cut or reconnect the simulated transport">
              <Scissors size={17} />
              {sim.partitioned ? 'Reconnect' : 'Partition'}
            </button>
            <button type="button" onClick={healNow} title="Reconnect and run anti-entropy immediately">
              <RefreshCw size={17} />
              Heal
            </button>
            <button type="button" onClick={() => setSim((current) => dropNextPacket(current))} disabled={sim.queue.length === 0} title="Drop the earliest queued delta">
              <Radio size={17} />
              Drop
            </button>
            <button type="button" onClick={() => setSim((current) => duplicateNextPacket(current))} disabled={sim.queue.length === 0} title="Duplicate the earliest queued delta">
              <Copy size={17} />
              Duplicate
            </button>
            <button type="button" onClick={() => setSim((current) => reorderQueue(current))} disabled={sim.queue.length < 2} title="Reverse queued delivery order">
              <Shuffle size={17} />
              Reorder
            </button>
            <button type="button" onClick={() => setSim(createSimulation(sim.peers.length))} title="Reset the simulation">
              <RotateCcw size={17} />
              Reset
            </button>
          </div>

          <div className="operation-bank">
            <label>
              Active replica
              <select value={selectedPeer} onChange={(event) => setSelectedPeer(Number(event.target.value))}>
                {sim.peers.map((peer) => (
                  <option key={peer.id} value={peer.id}>
                    Replica {peer.id}
                  </option>
                ))}
              </select>
            </label>
            <button type="button" onClick={() => setSim((current) => bumpCounter(current, selected.id))}>
              <Gauge size={17} />
              Append counter delta
            </button>
            <div className="supply-row">
              <input
                aria-label="Record element"
                list="supply-presets"
                value={element}
                onChange={(event) => setElement(event.target.value)}
                maxLength={22}
              />
              <datalist id="supply-presets">
                {SUPPLIES.map((supply) => (
                  <option key={supply} value={supply} />
                ))}
              </datalist>
              <button type="button" onClick={() => setSim((current) => addElement(current, selected.id, element))}>
                <CircleDot size={17} />
                Add
              </button>
              <button type="button" onClick={() => setSim((current) => removeElement(current, selected.id, element))}>
                <GitBranch size={17} />
                Remove seen
              </button>
            </div>
          </div>
        </aside>
      </section>

      <section className="telemetry-grid" aria-label="Telemetry">
        <div className="panel controls-panel">
          <h2>Transport tuning</h2>
          <label>
            Replicas
            <select
              value={sim.peers.length}
              onChange={(event) => {
                const count = Number(event.target.value)
                setSelectedPeer(0)
                setSim((current) => setPeerCount(current, count))
              }}
            >
              {[3, 4, 5, 6].map((count) => (
                <option key={count} value={count}>
                  {count}
                </option>
              ))}
            </select>
          </label>
          <label>
            Latency {sim.latencyMs}ms
            <input
              type="range"
              min="100"
              max="1800"
              step="50"
              value={sim.latencyMs}
              onChange={(event) => setSim((current) => setLatency(current, Number(event.target.value)))}
            />
          </label>
          <label>
            Drop rate {Math.round(sim.dropRate * 100)}%
            <input
              type="range"
              min="0"
              max="85"
              step="1"
              value={Math.round(sim.dropRate * 100)}
              onChange={(event) => setSim((current) => setDropRate(current, Number(event.target.value) / 100))}
            />
          </label>
          <label>
            Anti-entropy {sim.antiEntropyMs === 0 ? 'off' : `${(sim.antiEntropyMs / 1000).toFixed(0)}s`}
            <input
              type="range"
              min="0"
              max="15000"
              step="1000"
              value={sim.antiEntropyMs}
              onChange={(event) => setSim((current) => setAntiEntropyMs(current, Number(event.target.value)))}
            />
          </label>
          <label className="check-row">
            <input type="checkbox" checked={technical} onChange={(event) => setTechnical(event.target.checked)} />
            Technical labels
          </label>
          <label className="check-row">
            <input type="checkbox" checked={calmMotion} onChange={(event) => setCalmMotion(event.target.checked)} />
            Calm motion
          </label>
        </div>

        <div className="panel proof-panel">
          <header>
            <ShieldCheck size={18} />
            <h2>Honest claim boundary</h2>
          </header>
          <p>
            {technical
              ? 'The Lean-backed CRDT carriers prove convergence for the modeled state once replicas receive the same record set. This TypeScript view is a demo mirror, not the verified artifact.'
              : 'This demo makes the proof visible: drops, duplicates and reordering can delay a replica, then anti-entropy backfills the missing state.'}
          </p>
          <p>Not shown as proof: real network delivery, sensor truth, storage durability, or arbitrary user reducers.</p>
        </div>

        <div className="panel log-panel">
          <header>
            <BadgeCheck size={18} />
            <h2>{technical ? 'Raw transport log' : 'Story log'}</h2>
          </header>
          <ol>
            {sim.log.slice(0, 12).map((entry) => (
              <li key={entry.id} className={entry.tone}>
                <time>{(entry.at / 1000).toFixed(1)}s</time>
                <span>{technical ? entry.technical : entry.plain}</span>
              </li>
            ))}
          </ol>
        </div>
      </section>
    </main>
  )
}

function storyState(sim: Simulation, status: ReturnType<typeof convergence>) {
  if (sim.partitioned) {
    return {
      tone: 'warn',
      label: 'Split brain on purpose',
      detail: 'The partition is a modeled transport fault. Replicas can accept local records while links are cut.',
      Icon: Scissors,
    }
  }
  if (status.converged) {
    return {
      tone: 'ok',
      label: 'Every replica agrees',
      detail: 'The raw states match, the read model matches, and no queued packets remain.',
      Icon: BadgeCheck,
    }
  }
  return {
    tone: 'bad',
    label: 'Convergence pending',
    detail: sim.antiEntropyMs > 0 ? 'Some records are missing in transit. Anti-entropy can recover them.' : 'Anti-entropy is off, so dropped packets can remain missing.',
    Icon: Wifi,
  }
}

function sameRead(peer: Simulation['peers'][number], status: ReturnType<typeof convergence>): boolean {
  return readGCounter(peer.gcounter) === status.gcounterValue && readORSet(peer.orset).join('\u0000') === status.orsetElements.join('\u0000')
}

function digest(counter: number[], elements: string[]): string {
  return `G:${counter.join('.')} S:${elements.join('|') || '-'}`
}

function linkPairs(count: number): Array<[number, number]> {
  const pairs: Array<[number, number]> = []
  for (let left = 0; left < count; left += 1) {
    for (let right = left + 1; right < count; right += 1) pairs.push([left, right])
  }
  return pairs
}

function crossesPartition(left: number, right: number): boolean {
  return left < 2 !== right < 2
}

function peerPosition(index: number, count: number): { x: number; y: number } {
  const angle = -Math.PI / 2 + (index / count) * Math.PI * 2
  return {
    x: 50 + Math.cos(angle) * 30,
    y: 50 + Math.sin(angle) * 34,
  }
}

function packetPoint(packet: Packet, sim: Simulation, positions: Array<{ x: number; y: number }>): { x: number; y: number } {
  const from = positions[packet.from] ?? { x: 50, y: 50 }
  const to = positions[packet.to] ?? { x: 50, y: 50 }
  const progress = clamp((sim.now - packet.sentAt) / Math.max(1, packet.deliverAt - packet.sentAt), 0, 1)
  return {
    x: from.x + (to.x - from.x) * progress,
    y: from.y + (to.y - from.y) * progress,
  }
}

function packetLabel(packet: Packet): string {
  if (packet.delta.kind === 'gcounter.bump') return `G${packet.delta.replica}`
  if (packet.delta.kind === 'orset.add') return 'ADD'
  return 'REM'
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms))
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value))
}

export default App
