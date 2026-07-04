import { useEffect, useMemo, useState } from 'react'
import './App.css'
import {
  addElement,
  bumpCounter,
  convergence,
  createSimulation,
  removeElement,
  setDropRate,
  setLatency,
  setPartitioned,
  setPeerCount,
  tick,
  type Simulation,
} from './sim'
import { readGCounter, readORSet } from './crdt'

function App() {
  const [sim, setSim] = useState<Simulation>(() => createSimulation(4))
  const [selectedPeer, setSelectedPeer] = useState(0)
  const [element, setElement] = useState('medkit')
  const status = useMemo(() => convergence(sim), [sim])

  useEffect(() => {
    const timer = window.setInterval(() => {
      setSim((current) => tick(current, 250))
    }, 250)
    return () => window.clearInterval(timer)
  }, [])

  const selected = sim.peers[selectedPeer] ?? sim.peers[0]

  return (
    <main className="shell">
      <header className="topbar">
        <div>
          <p className="eyebrow">SafeMesh PWA MVP</p>
          <h1>Delta-state CRDT convergence lab</h1>
        </div>
        <div className={`status ${status.converged ? 'ok' : status.sameRawState ? 'syncing' : 'split'}`}>
          <strong>{status.converged ? 'Converged' : status.sameRawState ? 'States match' : 'Diverged'}</strong>
          <span>
            raw {status.sameRawState ? 'equal' : 'different'} / reads {status.sameReads ? 'equal' : 'different'} /{' '}
            {sim.queue.length} queued
          </span>
        </div>
      </header>

      <section className="controls" aria-label="Mesh controls">
        <label>
          Peers
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
          Actor
          <select value={selectedPeer} onChange={(event) => setSelectedPeer(Number(event.target.value))}>
            {sim.peers.map((peer) => (
              <option key={peer.id} value={peer.id}>
                Peer {peer.id}
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
          Drop {Math.round(sim.dropRate * 100)}%
          <input
            type="range"
            min="0"
            max="60"
            step="1"
            value={Math.round(sim.dropRate * 100)}
            onChange={(event) => setSim((current) => setDropRate(current, Number(event.target.value) / 100))}
          />
        </label>
        <button type="button" onClick={() => setSim((current) => setPartitioned(current, !current.partitioned))}>
          {sim.partitioned ? 'Heal' : 'Partition'}
        </button>
        <button type="button" onClick={() => setSim(createSimulation(sim.peers.length))}>
          Reset
        </button>
      </section>

      <section className="workspace">
        <div className="operations">
          <h2>Operations</h2>
          <div className="op-row">
            <button type="button" onClick={() => setSim((current) => bumpCounter(current, selected.id))}>
              Bump G-Counter
            </button>
            <span>local tally becomes {selected.localTally + 1}</span>
          </div>
          <div className="op-row split-input">
            <input
              aria-label="OR-Set element"
              value={element}
              onChange={(event) => setElement(event.target.value)}
              maxLength={22}
            />
            <button type="button" onClick={() => setSim((current) => addElement(current, selected.id, element))}>
              Add
            </button>
            <button type="button" onClick={() => setSim((current) => removeElement(current, selected.id, element))}>
              Remove Observed
            </button>
          </div>
        </div>

        <div className="graph" aria-label="Node graph">
          {sim.peers.map((peer) => (
            <button
              type="button"
              key={peer.id}
              className={`node ${peer.id === selectedPeer ? 'selected' : ''}`}
              onClick={() => setSelectedPeer(peer.id)}
            >
              <span>Peer {peer.id}</span>
              <b>{readGCounter(peer.gcounter)}</b>
            </button>
          ))}
          <div className={`linkline ${sim.partitioned ? 'partitioned' : ''}`}>
            {sim.partitioned ? 'partition active' : 'gossip links open'}
          </div>
        </div>
      </section>

      <section className="peers" aria-label="Peer states">
        {sim.peers.map((peer) => {
          const elements = readORSet(peer.orset)
          return (
            <article key={peer.id} className="peer">
              <header>
                <h2>Peer {peer.id}</h2>
                <span>{peer.id === selectedPeer ? 'actor' : 'replica'}</span>
              </header>
              <dl>
                <div>
                  <dt>G value</dt>
                  <dd>{readGCounter(peer.gcounter)}</dd>
                </div>
                <div>
                  <dt>G state</dt>
                  <dd>[{peer.gcounter.join(', ')}]</dd>
                </div>
                <div>
                  <dt>OR visible</dt>
                  <dd>{elements.length > 0 ? elements.join(', ') : 'empty'}</dd>
                </div>
                <div>
                  <dt>adds/tombs</dt>
                  <dd>
                    {Object.keys(peer.orset.adds).length}/{Object.keys(peer.orset.tombstones).length}
                  </dd>
                </div>
              </dl>
            </article>
          )
        })}
      </section>

      <section className="footer-grid">
        <div className="proof-note">
          <h2>Grounding</h2>
          <p>
            G-Counter mirrors Lean semantics that also ship in Rust and are differentially tested. OR-Set mirrors
            Lean-proven add-wins semantics directly; Rust OR-Set is still pending. This browser code is a demo mirror,
            not a verified artifact.
          </p>
          <p>
            Live check: value {status.gcounterValue}; OR-Set [{status.orsetElements.join(', ') || 'empty'}].
          </p>
        </div>
        <div className="event-log">
          <h2>Event log</h2>
          <ol>
            {sim.log.map((entry) => (
              <li key={entry.id} className={entry.tone}>
                <time>{(entry.at / 1000).toFixed(1)}s</time>
                {entry.text}
              </li>
            ))}
          </ol>
        </div>
      </section>
    </main>
  )
}

export default App
