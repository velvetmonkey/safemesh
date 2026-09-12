import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ComponentType } from 'react'
import {
  BadgeCheck,
  ChevronLeft,
  ChevronRight,
  CircleDot,
  Copy,
  Equal,
  EqualNot,
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
import {
  GUIDE_SCENARIOS,
  readORSet,
  addElement,
  advanceScenarioSimulation,
  buildScenarioSimulation,
  bumpCounter,
  clampScenarioStep,
  convergence,
  createSimulation,
  currentScenario,
  currentScenarioStep,
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
  type GuideScenario,
  type LogEntry,
  type Packet,
  type Simulation,
} from './sim'

const DEFAULT_SCENARIO_ID = GUIDE_SCENARIOS[0].id
const SUPPLIES = ['vaccine', 'freezer', 'medkit', 'insulin', 'water', 'rations', 'fuel', 'radio', 'generator', 'antibiotics']

function App() {
  const [activeScenarioId, setActiveScenarioId] = useState(DEFAULT_SCENARIO_ID)
  const [scenarioSim, setScenarioSim] = useState<Simulation>(() => buildScenarioSimulation(DEFAULT_SCENARIO_ID, 0))
  const [stepIndex, setStepIndex] = useState(0)
  const [stepElapsed, setStepElapsed] = useState(() => currentScenarioStep(DEFAULT_SCENARIO_ID, 0).motionMs)
  const [stepAnimating, setStepAnimating] = useState(false)
  const [advancedOpen, setAdvancedOpen] = useState(false)
  const [sandboxSim, setSandboxSim] = useState<Simulation>(() => createSimulation(4))
  const [selectedPeer, setSelectedPeer] = useState(0)
  const [element, setElement] = useState('vaccine')
  const [technical, setTechnical] = useState(false)
  const [calmMotion, setCalmMotion] = useState(() => window.matchMedia('(prefers-reduced-motion: reduce)').matches)
  const [compactStage, setCompactStage] = useState(() => window.matchMedia('(max-width: 760px)').matches)

  const fieldViewport = useRef<HTMLDivElement>(null)
  const field = useRef<HTMLElement>(null)
  const [fieldSize, setFieldSize] = useState({ width: 0, height: 0 })

  const activeScenario = currentScenario(activeScenarioId)
  const activeStep = currentScenarioStep(activeScenarioId, stepIndex)
  const sim = advancedOpen ? sandboxSim : scenarioSim
  const status = useMemo(() => convergence(sim), [sim])
  const positions = useMemo(
    () => Array.from({ length: sim.peers.length }, (_, index) => peerPosition(index, sim.peers.length, compactStage)),
    [compactStage, sim.peers.length],
  )
  const selected = sim.peers[selectedPeer] ?? sim.peers[0]
  const recentAntiEntropy = sim.lastAntiEntropyAt !== null && sim.now - sim.lastAntiEntropyAt < 1500
  const story = storyState(sim, status)
  const StoryIcon = story.Icon
  const stepProgress = activeStep.motionMs === 0 ? 100 : Math.round((stepElapsed / activeStep.motionMs) * 100)
  const stepStartTime = useMemo(() => buildScenarioSimulation(activeScenarioId, stepIndex, 0).now, [activeScenarioId, stepIndex])
  const visibleLog = useMemo(() => eventLogForMode(sim.log, advancedOpen, stepStartTime), [advancedOpen, sim.log, stepStartTime])

  // Absolute nodes do not contribute to intrinsic stage size. Measure their
  // actual content, then reserve enough room for every node and its focus ring.
  useLayoutEffect(() => {
    const viewport = fieldViewport.current
    const stage = field.current
    if (!viewport || !stage) return
    const measure = () => {
      const width = viewport.clientWidth
      const height = parseFloat(getComputedStyle(stage).getPropertyValue('--stage-height'))
      if (!width || !height) return
      const cards = Array.from(stage.querySelectorAll<HTMLElement>('.replica'))
      const core = stage.querySelector<HTMLElement>('.convergence-core')!
      const nodes = [...cards.map((card, i) => ({
        x: positions[i].x / 100, y: positions[i].y / 100,
        width: card.offsetWidth, height: card.offsetHeight,
      })), { x: 0.5, y: 0.5, width: core.offsetWidth, height: core.offsetHeight }]
      let requiredWidth = width, requiredHeight = height
      for (const node of nodes) {
        requiredWidth = Math.max(requiredWidth, (node.width / 2 + 24) / Math.min(node.x, 1 - node.x))
        requiredHeight = Math.max(requiredHeight, (node.height / 2 + 24) / Math.min(node.y, 1 - node.y))
      }
      for (let i = 0; i < nodes.length; i++) {
        for (let j = i + 1; j < nodes.length; j++) {
          const a = nodes[i], b = nodes[j]
          // Prefer vertical growth so a small spacing deficit does not add a
          // desktop scrollbar. Nodes sharing a row must separate horizontally.
          const separatedWidth = ((a.width + b.width) / 2 + 24) / Math.abs(a.x - b.x)
          const separatedHeight = ((a.height + b.height) / 2 + 24) / Math.abs(a.y - b.y)
          if (separatedWidth > requiredWidth && separatedHeight > requiredHeight) {
            if (Math.abs(a.y - b.y) < 0.001) requiredWidth = separatedWidth
            else requiredHeight = separatedHeight
          }
        }
      }
      const next = { width: Math.ceil(requiredWidth), height: Math.ceil(requiredHeight) }
      setFieldSize(previous => previous.width === next.width && previous.height === next.height ? previous : next)
    }
    const observer = new ResizeObserver(measure)
    observer.observe(viewport)
    stage.querySelectorAll('.replica, .convergence-core').forEach(node => observer.observe(node))
    window.addEventListener('resize', measure)
    measure()
    return () => { observer.disconnect(); window.removeEventListener('resize', measure) }
  }, [positions])

  useEffect(() => {
    const media = window.matchMedia('(max-width: 760px)')
    const update = () => setCompactStage(media.matches)
    update()
    media.addEventListener('change', update)
    return () => media.removeEventListener('change', update)
  }, [])

  useEffect(() => {
    const frameMs = 33
    const simDtPerFrame = calmMotion ? 9 : 20
    const timer = window.setInterval(() => {
      if (advancedOpen) {
        setSandboxSim((current) => tick(current, simDtPerFrame))
        return
      }

      if (!stepAnimating) return

      const remaining = activeStep.motionMs - stepElapsed
      if (remaining <= 0) {
        setStepAnimating(false)
        return
      }

      const stepMs = Math.min(simDtPerFrame, remaining)
      setScenarioSim((current) => advanceScenarioSimulation(current, stepMs))
      setStepElapsed((current) => Math.min(activeStep.motionMs, current + stepMs))
      if (stepMs >= remaining) setStepAnimating(false)
    }, frameMs)

    return () => window.clearInterval(timer)
  }, [advancedOpen, activeStep.motionMs, calmMotion, stepAnimating, stepElapsed])

  function loadScenario(scenarioId: string, targetStep = 0, options: { play?: boolean; complete?: boolean } = {}) {
    const scenario = currentScenario(scenarioId)
    const nextStepIndex = clampScenarioStep(scenario.id, targetStep)
    const nextStep = currentScenarioStep(scenario.id, nextStepIndex)
    const elapsed = options.complete ? nextStep.motionMs : 0
    setAdvancedOpen(false)
    setSelectedPeer(0)
    setActiveScenarioId(scenario.id)
    setStepIndex(nextStepIndex)
    setStepElapsed(elapsed)
    setScenarioSim(buildScenarioSimulation(scenario.id, nextStepIndex, elapsed))
    setStepAnimating(Boolean(options.play) && nextStep.motionMs > elapsed)
  }

  function loadScenarioStep(targetStep: number, options: { play?: boolean; complete?: boolean } = {}) {
    loadScenario(activeScenarioId, targetStep, options)
  }

  function toggleStepPlayback() {
    if (stepAnimating) {
      setStepAnimating(false)
      return
    }

    if (stepElapsed > 0 && stepElapsed < activeStep.motionMs) {
      setStepAnimating(true)
      return
    }

    loadScenarioStep(stepIndex, { play: true })
  }

  function toggleAdvancedLab() {
    setAdvancedOpen((open) => !open)
    setStepAnimating(false)
    setSelectedPeer(0)
  }

  function healNow() {
    setSandboxSim((current) => runAntiEntropyNow(setPartitioned(current, false)))
  }

  return (
    <main className={`control-room ${calmMotion ? 'calm-motion' : ''}`}>
      <header className="topbar">
        <div>
          <p className="eyebrow">SafeMesh for builders</p>
          <h1>Same message. Five failures. Same finish.</h1>
        </div>
        <div className={`state-pill ${story.tone}`} aria-live="polite">
          <StoryIcon aria-hidden="true" size={18} />
          <span>{story.label}</span>
        </div>
      </header>

      <section className="scenario-strip" aria-label="Choose what can go wrong">
        <div className="scenario-intro">
          <p className="eyebrow">Choose what can go wrong</p>
          <h2>Every case sends vaccine plus audit count 1, then proves the same modeled finish.</h2>
        </div>
        <div className="scenario-buttons">
          {GUIDE_SCENARIOS.map((scenario) => {
            const ScenarioIcon = scenarioIcon(scenario.id)
            return (
              <button
                type="button"
                key={scenario.id}
                className={`scenario-button ${!advancedOpen && scenario.id === activeScenarioId ? 'active' : ''}`}
                onClick={() => loadScenario(scenario.id)}
              >
                <ScenarioIcon size={17} />
                <span>
                  <strong>{scenario.label}</strong>
                  <small>{scenario.headline}</small>
                </span>
              </button>
            )
          })}
        </div>
      </section>

      <section className="demo-grid" aria-label="Interactive convergence demo">
        <div className="stage-shell">
          {!advancedOpen && <div className={`stage-cue ${activeStep.tone}`}>{activeStep.stageCue ?? activeStep.watchFor}</div>}
          <div className="stage-viewport" ref={fieldViewport} tabIndex={0} role="region" aria-label="Scrollable replica diagram">
          <section className="stage" ref={field} style={{ width: fieldSize.width || undefined, minHeight: fieldSize.height || undefined }} aria-label="Replica field">
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

            {sim.queue.slice(0, compactStage ? 8 : 18).map((packet) => {
              const point = packetPoint(packet, sim, positions)
              return (
                <div
                  key={packet.id}
                  className={`packet ${packet.delta.kind.replace('.', '-')} ${packet.phase ?? 'normal'} ${packet.duplicated ? 'duplicated' : ''}`}
                  style={{ left: `${point.x}%`, top: `${point.y}%` }}
                  title={`record / log bytes ${wirePreview(packet.delta)}`}
                >
                  <span>{packetLabel(packet, sim.queue)}</span>
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
              const isSelected = peer.id === selectedPeer
              // Rust-core carrier comparison against Replica 0, the same fact the CONVERGED core aggregates.
              const aligned = status.rawStateMatches[peer.id] ?? false
              const StateIcon = aligned ? Equal : EqualNot
              return (
                <button
                  type="button"
                  key={peer.id}
                  className={`replica ${isSelected ? 'selected' : ''} ${aligned ? 'aligned' : 'split'}`}
                  style={{ left: `${position.x}%`, top: `${position.y}%` }}
                  aria-pressed={isSelected}
                  onClick={() => setSelectedPeer(peer.id)}
                >
                  <span className="replica-kicker">
                    <span>Replica {peer.id}</span>
                    <span className="replica-state" title={aligned ? 'Core state matches Replica 0' : 'Core state differs from Replica 0'}>
                      <StateIcon aria-hidden="true" size={13} />
                      <span className="visually-hidden">{aligned ? 'aligned' : 'split'}</span>
                    </span>
                  </span>
                  <span>G-Counter · Rust/WASM</span>
                  <strong>{status.gcounterValues[peer.id] ?? 0}</strong>
                  <span>OR-Set · Rust/WASM</span>
                  <span>{elements.length > 0 ? elements.join(', ') : 'no records yet'}</span>
                  <code>{digest(peer.gcounter, elements)}</code>
                </button>
              )
            })}
          </section>
          </div>

          <div className="stage-step-controls" aria-label="Scenario step controls">
            {advancedOpen ? (
              <span className="lab-stage-note">Advanced lab is live. Use the controls below to fault the visible mesh.</span>
            ) : (
              <>
                <button type="button" onClick={() => loadScenarioStep(stepIndex - 1, { complete: true })} disabled={stepIndex === 0}>
                  <ChevronLeft size={17} />
                  Previous
                </button>
                <button type="button" onClick={toggleStepPlayback}>
                  {stepAnimating ? <Pause size={17} /> : <Play size={17} />}
                  {stepAnimating ? 'Pause' : stepElapsed > 0 && stepElapsed < activeStep.motionMs ? 'Resume' : 'Replay step'}
                </button>
                <button type="button" onClick={() => loadScenarioStep(stepIndex + 1, { play: true })} disabled={stepIndex === activeScenario.steps.length - 1}>
                  Next
                  <ChevronRight size={17} />
                </button>
                <button type="button" onClick={() => loadScenario(activeScenarioId)}>
                  <RotateCcw size={17} />
                  Restart
                </button>
                <span className="step-progress">{stepProgress}% watched</span>
              </>
            )}
          </div>
        </div>

        <div className="story-row">
          <aside className="narrative-panel" aria-label="Narrative arc">
            {advancedOpen ? (
              <>
                <p className="eyebrow">Advanced lab</p>
                <h2>{story.label}</h2>
                <p>{story.detail}</p>
                <div className="watch-card">
                  <strong>Current state</strong>
                  <span>{sandboxReadout(sim, status)}</span>
                </div>
              </>
            ) : (
              <>
                <p className="eyebrow">
                  Scenario: {activeScenario.label} · Step {stepIndex + 1} of {activeScenario.steps.length}
                </p>
                <h2>{activeStep.title}</h2>
                <p>{activeStep.plainExplanation}</p>
                <div className="watch-card">
                  <strong>Setup</strong>
                  <span>{activeScenario.setup}</span>
                </div>
                <div className="watch-card">
                  <strong>Why it matters</strong>
                  <span>{activeScenario.whyItMatters}</span>
                </div>
                <div className="watch-card">
                  <strong>Watch</strong>
                  <span>{activeStep.watchFor}</span>
                </div>
                <div className="watch-card success">
                  <strong>Finish line</strong>
                  <span>{activeScenario.successState}</span>
                </div>
              </>
            )}
            <p className="claim-line">The G-Counter and OR-Set use Rust-core records, merges, and reads through WASM. The Lean-backed Rust core is the proof-carrying surface for modeled CRDT semantics.</p>
          </aside>

          <aside className="log-panel" aria-label={advancedOpen ? 'Story log' : 'Events in this step'}>
            <header>
              <BadgeCheck size={18} />
              <h2>{advancedOpen ? (technical ? 'Raw transport log' : 'Story log') : 'Events in this step'}</h2>
            </header>
            <ol>
              {visibleLog.length === 0 ? (
                <li className="empty-log">
                  <time>--</time>
                  <span>No transport event in this step yet.</span>
                </li>
              ) : (
                visibleLog.map((entry) => (
                  <li key={entry.id} className={entry.tone}>
                    <time>{(entry.at / 1000).toFixed(1)}s</time>
                    <span>{technical ? entry.technical : entry.plain}</span>
                  </li>
                ))
              )}
            </ol>
          </aside>
        </div>
      </section>

      <section className={`control-dock ${advancedOpen ? 'open' : ''}`} aria-label="Advanced controls">
        <div className="dock-footer">
          <button type="button" className={`advanced-toggle ${advancedOpen ? 'active' : ''}`} onClick={toggleAdvancedLab}>
            <Gauge size={17} />
            {advancedOpen ? 'Close advanced lab' : 'Open advanced lab'}
          </button>
          <label className="check-row">
            <input type="checkbox" checked={technical} onChange={(event) => setTechnical(event.target.checked)} />
            Technical labels
          </label>
          <label className="check-row">
            <input type="checkbox" checked={calmMotion} onChange={(event) => setCalmMotion(event.target.checked)} />
            Calm motion
          </label>
          <div className="proof-note">
            <ShieldCheck size={18} />
            <span>
              {technical
                ? 'Lean backs the CRDT semantics; the G-Counter and OR-Set use Rust-core records, merges, and reads through WASM.'
                : 'Shown: modeled convergence after records meet. Not shown as proof: real network delivery, sensor truth, storage durability, or arbitrary reducers.'}
            </span>
          </div>
        </div>

        {advancedOpen && (
          <div className="sandbox-controls">
            <div className="control-bank" aria-label="Fault controls">
              <button type="button" onClick={() => setSandboxSim((current) => setPartitioned(current, !current.partitioned))} title="Cut or reconnect the simulated transport">
                <Scissors size={17} />
                {sandboxSim.partitioned ? 'Reconnect' : 'Partition'}
              </button>
              <button type="button" onClick={healNow} title="Reconnect and run anti-entropy immediately">
                <RefreshCw size={17} />
                Heal
              </button>
              <button type="button" onClick={() => setSandboxSim((current) => dropNextPacket(current))} disabled={sandboxSim.queue.length === 0} title="Drop the earliest queued delta">
                <Radio size={17} />
                Drop
              </button>
              <button type="button" onClick={() => setSandboxSim((current) => duplicateNextPacket(current))} disabled={sandboxSim.queue.length === 0} title="Duplicate the earliest queued delta">
                <Copy size={17} />
                Duplicate
              </button>
              <button type="button" onClick={() => setSandboxSim((current) => reorderQueue(current))} disabled={sandboxSim.queue.length < 2} title="Reverse queued delivery order">
                <Shuffle size={17} />
                Reorder
              </button>
              <button type="button" onClick={() => setSandboxSim(createSimulation(sandboxSim.peers.length))} title="Reset the simulation">
                <RotateCcw size={17} />
                Reset
              </button>
            </div>

            <div className="operation-bank">
              <label>
                Active replica
                <select value={selectedPeer} onChange={(event) => setSelectedPeer(Number(event.target.value))}>
                  {sandboxSim.peers.map((peer) => (
                    <option key={peer.id} value={peer.id}>
                      Replica {peer.id}
                    </option>
                  ))}
                </select>
              </label>
              <button type="button" onClick={() => setSandboxSim((current) => bumpCounter(current, selected.id))}>
                <Gauge size={17} />
                Append counter delta
              </button>
              <div className="supply-row">
                <select aria-label="Record element" value={element} onChange={(event) => setElement(event.target.value)}>
                  {SUPPLIES.map((supply) => (
                    <option key={supply} value={supply}>
                      {supply}
                    </option>
                  ))}
                </select>
                <button type="button" onClick={() => setSandboxSim((current) => addElement(current, selected.id, element))}>
                  <CircleDot size={17} />
                  Add
                </button>
                <button type="button" onClick={() => setSandboxSim((current) => removeElement(current, selected.id, element))}>
                  <GitBranch size={17} />
                  Remove seen
                </button>
              </div>
            </div>

            <div className="tuning-bank">
              <label>
                Replicas
                <select
                  value={sandboxSim.peers.length}
                  onChange={(event) => {
                    const count = Number(event.target.value)
                    setSelectedPeer(0)
                    setSandboxSim((current) => setPeerCount(current, count))
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
                Latency {sandboxSim.latencyMs}ms
                <input
                  type="range"
                  min="100"
                  max="1800"
                  step="50"
                  value={sandboxSim.latencyMs}
                  onChange={(event) => setSandboxSim((current) => setLatency(current, Number(event.target.value)))}
                />
              </label>
              <label>
                Drop rate {Math.round(sandboxSim.dropRate * 100)}%
                <input
                  type="range"
                  min="0"
                  max="85"
                  step="1"
                  value={Math.round(sandboxSim.dropRate * 100)}
                  onChange={(event) => setSandboxSim((current) => setDropRate(current, Number(event.target.value) / 100))}
                />
              </label>
              <label>
                Anti-entropy {sandboxSim.antiEntropyMs === 0 ? 'off' : `${(sandboxSim.antiEntropyMs / 1000).toFixed(0)}s`}
                <input
                  type="range"
                  min="0"
                  max="15000"
                  step="1000"
                  value={sandboxSim.antiEntropyMs}
                  onChange={(event) => setSandboxSim((current) => setAntiEntropyMs(current, Number(event.target.value)))}
                />
              </label>
            </div>
          </div>
        )}
      </section>
    </main>
  )
}

function storyState(sim: Simulation, status: ReturnType<typeof convergence>) {
  if (sim.partitioned) {
    return {
      tone: 'warn',
      label: 'Split on purpose',
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

function peerPosition(index: number, count: number, compact: boolean): { x: number; y: number } {
  if (compact && count === 4) {
    return [
      { x: 50, y: 15 },
      { x: 83, y: 50 },
      { x: 50, y: 85 },
      { x: 17, y: 50 },
    ][index]
  }

  const angle = -Math.PI / 2 + (index / count) * Math.PI * 2
  return {
    x: 50 + Math.cos(angle) * (compact ? 34 : 30),
    y: 50 + Math.sin(angle) * (compact ? 36 : 34),
  }
}

function packetPoint(packet: Packet, sim: Simulation, positions: Array<{ x: number; y: number }>): { x: number; y: number } {
  const from = positions[packet.from] ?? { x: 50, y: 50 }
  const to = positions[packet.to] ?? { x: 50, y: 50 }
  const progress = clamp((sim.now - packet.sentAt) / Math.max(1, packet.deliverAt - packet.sentAt), 0, 1)
  const displayProgress = clamp(progress + packetProgressStagger(packet), 0.08, 0.92)
  const dx = to.x - from.x
  const dy = to.y - from.y
  const length = Math.hypot(dx, dy) || 1
  const lane = packetLane(packet)
  const perpendicular = { x: -dy / length, y: dx / length }

  return {
    x: clamp(from.x + dx * displayProgress + perpendicular.x * lane, 6, 94),
    y: clamp(from.y + dy * displayProgress + perpendicular.y * lane, 8, 92),
  }
}

function packetLane(packet: Packet): number {
  const kindLane = packet.delta.kind === 'gcounter.bump' ? -2.4 : packet.delta.kind === 'orset.add' ? 2.4 : 0
  const destinationLane = ((packet.to + packet.from) % 3) - 1
  const duplicateLane = packet.duplicated ? 2.2 : 0
  const reorderLane = packet.phase === 'reordered' ? ((packet.order ?? 1) - 3.5) * 3.2 : 0
  return (kindLane + destinationLane * 1.4 + duplicateLane + reorderLane) * 1.9
}

function packetProgressStagger(packet: Packet): number {
  if (packet.phase === 'reordered') return (3.5 - (packet.order ?? 1)) * 0.13
  const kindShift = packet.delta.kind === 'gcounter.bump' ? -0.07 : packet.delta.kind === 'orset.add' ? 0.07 : 0
  const destinationShift = (((packet.to + packet.from) % 3) - 1) * 0.025
  const duplicateShift = packet.duplicated ? 0.09 : 0
  return kindShift + destinationShift + duplicateShift
}

function packetLabel(packet: Packet, queue: Packet[]): string {
  const deltaLabel = packet.delta.kind === 'gcounter.bump' ? `G${packet.delta.replica}` : packet.delta.kind === 'orset.add' ? 'ADD' : packet.delta.kind === 'orset.log' ? 'OR' : 'REM'
  if (packet.phase === 'repair') return `SYNC ${deltaLabel}`
  const rank = packet.order ?? deliveryRank(packet, queue)
  if (packet.phase === 'reordered') return `REV ${rank} ${deltaLabel}`
  return `#${rank} ${deltaLabel}`
}

function deliveryRank(packet: Packet, queue: Packet[]): number {
  return (
    [...queue]
      .sort((left, right) => left.deliverAt - right.deliverAt || left.id.localeCompare(right.id))
      .findIndex((candidate) => candidate.id === packet.id) + 1
  )
}

function eventLogForMode(log: LogEntry[], advancedOpen: boolean, stepStartTime: number): LogEntry[] {
  if (advancedOpen) return log.slice(0, 14)
  return log
    .filter((entry) => entry.at >= stepStartTime)
    .sort((left, right) => left.at - right.at)
    .slice(-14)
}

function sandboxReadout(sim: Simulation, status: ReturnType<typeof convergence>): string {
  if (status.converged) return `All ${sim.peers.length} replicas hold the same modeled state.`
  if (sim.partitioned) return 'The mesh is partitioned; local writes can diverge until the link heals.'
  return `${sim.queue.length} packets are still moving or missing from at least one replica.`
}

function scenarioIcon(scenarioId: GuideScenario['id']): ComponentType<{ size?: number }> {
  if (scenarioId === 'duplicate') return Copy
  if (scenarioId === 'reorder') return Shuffle
  if (scenarioId === 'drop') return Radio
  if (scenarioId === 'partition') return Scissors
  return BadgeCheck
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value))
}

export default App
