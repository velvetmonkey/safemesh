import { useEffect, useMemo, useState } from 'react'
import './App.css'
import { readGCounter, readORSet } from './crdt'
import {
  addElement,
  bumpCounter,
  convergence,
  createSimulation,
  removeElement,
  runAntiEntropyNow,
  setAntiEntropyMs,
  setDropRate,
  setLatency,
  setPartitioned,
  setPeerCount,
  tick,
  type Simulation,
} from './sim'

const SUPPLIES = ['medkit', 'flour', 'biscuits', 'water', 'radio', 'cash']

function App() {
  const [sim, setSim] = useState<Simulation>(() => createSimulation(4))
  const [selectedPeer, setSelectedPeer] = useState(0)
  const [element, setElement] = useState('medkit')
  const [technical, setTechnical] = useState(false)
  const [introOpen, setIntroOpen] = useState(() => !window.localStorage.getItem('safemesh-intro-dismissed'))
  const [tourCaption, setTourCaption] = useState('')
  const [tourRunning, setTourRunning] = useState(false)
  const status = useMemo(() => convergence(sim), [sim])

  useEffect(() => {
    const timer = window.setInterval(() => {
      setSim((current) => tick(current, 250))
    }, 250)
    return () => window.clearInterval(timer)
  }, [])

  const selected = sim.peers[selectedPeer] ?? sim.peers[0]
  const banner = bannerState(sim, status)
  const title = technical
    ? `${sim.peers.length} peers, simulated transport. Watch delta-state CRDTs converge.`
    : `${sim.peers.length} camps, no internet. Watch them stay in sync anyway.`

  async function runGuidedTour() {
    if (tourRunning) return
    setTourRunning(true)
    let scripted = setDropRate(setAntiEntropyMs(createSimulation(4), 5000), 1)
    scripted = setLatency(scripted, 500)
    setSelectedPeer(0)

    const steps: Array<[string, (current: Simulation) => Simulation, number]> = [
      ['Camp 0 adds medkit. The signal to neighbours is deliberately unreliable.', (s) => addElement(s, 0, 'medkit'), 850],
      ['The first radio delivery drops. One camp is missing medkit for now.', (s) => tick(s, 1200, () => 0), 950],
      ['Cut the network. Camps can keep working while split.', (s) => setPartitioned(s, true), 850],
      ['Camp 1 adds flour on its side of the split.', (s) => addElement(s, 1, 'flour'), 850],
      ['Camp 2 raises the headcount while still cut off.', (s) => bumpCounter(s, 2), 850],
      ['Reconnect. Anti-entropy exchanges state digests and backfills what was missed.', (s) => setPartitioned(s, false), 850],
      ['One anti-entropy round runs: missing supplies and headcount are recovered.', (s) => runAntiEntropyNow(s), 1100],
    ]

    for (const [caption, update, wait] of steps) {
      setTourCaption(caption)
      scripted = update(scripted)
      setSim(scripted)
      await sleep(wait)
    }

    setTourCaption('All camps agree again. Drops caused delay, not permanent divergence.')
    setTourRunning(false)
  }

  return (
    <main className="shell">
      <header className="hero">
        <div>
          <p className="eyebrow">SafeMesh supply-run demo</p>
          <h1>{title}</h1>
        </div>
        <a className="proof-badge" href="./README.md" title="Lean proves the CRDT convergence properties. This browser UI is only a TypeScript demo mirror.">
          Convergence proven in Lean 4
        </a>
      </header>

      <section className={`banner ${banner.tone}`} aria-live="polite">
        <strong>
          <span aria-hidden="true">{banner.icon}</span> {banner.title}
        </strong>
        <span>{banner.detail}</span>
      </section>

      {introOpen && (
        <section className="intro">
          <div>
            <h2>Try three moves</h2>
            <p>Add supplies, raise a camp headcount, then cut and reconnect the network to watch missing state catch up.</p>
          </div>
          <button
            type="button"
            onClick={() => {
              window.localStorage.setItem('safemesh-intro-dismissed', '1')
              setIntroOpen(false)
            }}
          >
            Got it
          </button>
        </section>
      )}

      <section className="controls" aria-label="Supply-run controls">
        <label>
          {technical ? 'Peers' : 'Camps'}
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
          <span>{technical ? 'replica count' : 'how many radios are sharing notes'}</span>
        </label>
        <label>
          {technical ? 'Actor peer' : 'Active camp'}
          <select value={selectedPeer} onChange={(event) => setSelectedPeer(Number(event.target.value))}>
            {sim.peers.map((peer) => (
              <option key={peer.id} value={peer.id}>
                {technical ? `Peer ${peer.id}` : `Camp ${peer.id}`}
              </option>
            ))}
          </select>
          <span>{technical ? 'source replica' : 'where the next action happens'}</span>
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
          <span>radio delay</span>
        </label>
        <label>
          Drop {Math.round(sim.dropRate * 100)}%
          <input
            type="range"
            min="0"
            max="80"
            step="1"
            value={Math.round(sim.dropRate * 100)}
            onChange={(event) => setSim((current) => setDropRate(current, Number(event.target.value) / 100))}
          />
          <span>{sim.antiEntropyMs > 0 ? 'converges anyway with anti-entropy' : 'can diverge if disabled'}</span>
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
          <span>{technical ? 'state-digest merge interval' : 'catch-up radio check'}</span>
        </label>
        <button type="button" onClick={() => setSim((current) => setPartitioned(current, !current.partitioned))}>
          {sim.partitioned ? 'Reconnect' : 'Cut network'}
        </button>
        <button type="button" onClick={() => setSim(createSimulation(sim.peers.length))}>
          Reset
        </button>
      </section>

      <section className="actions">
        <div className="operations">
          <h2>{technical ? 'CRDT operations' : 'Supply-run actions'}</h2>
          <div className="op-row">
            <button type="button" onClick={() => setSim((current) => bumpCounter(current, selected.id))}>
              {technical ? 'Bump G-Counter' : 'Raise headcount'}
            </button>
            <span>{technical ? `coordinate ${selected.id} becomes ${selected.localTally + 1}` : `Camp ${selected.id} count becomes ${selected.localTally + 1}`}</span>
          </div>
          <div className="op-row split-input">
            <input
              aria-label="Supply item"
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
              {technical ? 'OR-Set add' : 'Add supply'}
            </button>
            <button type="button" onClick={() => setSim((current) => removeElement(current, selected.id, element))}>
              {technical ? 'OR-Set remove observed' : 'Remove seen supply'}
            </button>
          </div>
        </div>

        <div className="tour-panel">
          <div>
            <h2>Guided try-this</h2>
            <p>{tourCaption || 'Run a scripted drop, split, reconnect, and anti-entropy recovery.'}</p>
          </div>
          <button type="button" disabled={tourRunning} onClick={runGuidedTour}>
            {tourRunning ? 'Running...' : 'Try this'}
          </button>
        </div>
      </section>

      <section className="map" aria-label="Camp map">
        {sim.peers.map((peer) => {
          const elements = readORSet(peer.orset)
          return (
            <button
              type="button"
              key={peer.id}
              className={`camp ${peer.id === selectedPeer ? 'selected' : ''}`}
              onClick={() => setSelectedPeer(peer.id)}
            >
              <span className="camp-title">{technical ? `Peer ${peer.id}` : `Camp ${peer.id}`}</span>
              <span className="headcount">{readGCounter(peer.gcounter)}</span>
              <span className="supplies">{elements.length > 0 ? elements.join(', ') : 'no supplies yet'}</span>
            </button>
          )
        })}
        <div className={`network-line ${sim.partitioned ? 'cut' : ''}`}>
          {sim.partitioned ? 'broken radio links' : 'connected radio links'}
        </div>
      </section>

      <section className="details-grid">
        <div className="proof-note">
          <header>
            <h2>{technical ? 'Grounding' : 'What is being shown'}</h2>
            <label className="toggle">
              <input type="checkbox" checked={technical} onChange={(event) => setTechnical(event.target.checked)} />
              Technical view
            </label>
          </header>
          <p>
            {technical
              ? 'G-Counter mirrors Lean semantics that also ship in Rust and are differentially tested. OR-Set mirrors Lean-proven add-wins semantics directly; Rust OR-Set is still pending. This browser code is a demo mirror, not a verified artifact.'
              : 'Each camp keeps its own notes. Even with dropped radio messages, the catch-up check merges what neighbours know until every camp agrees again.'}
          </p>
          <p>
            {technical
              ? 'Anti-entropy uses merge_deltaState: merging replicas is receiving the union of their delta sets. delta_dissemination_sec covers order and duplicate delivery.'
              : `Live check: headcount ${status.gcounterValue}; supplies [${status.orsetElements.join(', ') || 'empty'}].`}
          </p>
        </div>
        <div className="event-log">
          <header>
            <h2>{technical ? 'Event log' : 'Timeline'}</h2>
            <span>{technical ? 'raw transport terms' : 'plain language'}</span>
          </header>
          <ol>
            {sim.log.map((entry) => (
              <li key={entry.id} className={entry.tone}>
                <time>{(entry.at / 1000).toFixed(1)}s</time>
                {technical ? entry.technical : entry.plain}
              </li>
            ))}
          </ol>
        </div>
      </section>
    </main>
  )
}

function bannerState(sim: Simulation, status: ReturnType<typeof convergence>) {
  if (sim.partitioned) {
    return {
      tone: 'warn',
      icon: '◐',
      title: 'Network split',
      detail: `${sim.peers.length} camps can keep working, but radio messages wait for reconnect.`,
    }
  }
  if (status.converged) {
    return {
      tone: 'ok',
      icon: '✓',
      title: `All ${sim.peers.length} camps agree`,
      detail: 'Raw state and visible supplies match everywhere.',
    }
  }
  return {
    tone: 'bad',
    icon: '✗',
    title: 'Camps disagree',
    detail: sim.antiEntropyMs > 0 ? 'A catch-up round will backfill dropped messages.' : 'Anti-entropy is off.',
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms))
}

export default App
