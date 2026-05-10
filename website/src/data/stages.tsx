/**
 * 10 stages, modeled on tiny-dec's stage list. The first stage is the hero,
 * the last is the strengths + funding outro. Everything in between is a tour
 * stop keyed to the IDE mock and its animation script.
 *
 * `target` and `side` describe where the colored ring sits on the IDE mock;
 * the IDE mock owns its animation via the `anim` field, resolved inside
 * IdeMock.tsx. The `body` field carries rich JSX so each rail entry can have
 * its own structured headings + bullet lists.
 */

export type Side = 'top' | 'right' | 'bottom' | 'left';

/** Animation keys consumed by IdeMock.tsx */
export type AnimKey =
  | 'idle'
  | 'continue'
  | 'step-into'
  | 'reverse'
  | 'workspace'
  | 'breakpoints'
  | 'locals'
  | 'watch'
  | 'decompile-soon'
  | 'snapshot-cycle';

export type StageKind = 'panel' | 'tour';

export type Stage = {
  id: string;
  kind: StageKind;
  /** label shown on the progress strip */
  label: string;
  /** marketing badge shown above the rail title */
  badge: string;
  /** rail headline (one short phrase) */
  title: string;
  /** rich rail body: section headings, bullets, tips */
  body: React.ReactNode;
  /** accent color used for the ring + rail badge + section headings */
  color: string;
  /** for kind === 'tour': which IDE mock element to ring */
  target?:
    | { kind: 'selector'; selector: string }
    | { kind: 'whole-frame' };
  /** for kind === 'tour': which side the ring lives on */
  side?: Side;
  /** for kind === 'tour': animation script to run while this stage is active */
  anim?: AnimKey;
};

/* small helpers used inside body JSX */

function Sec({ children }: { children: React.ReactNode }) {
  return <h3 className="rail-sec">{children}</h3>;
}
function Tip({ children }: { children: React.ReactNode }) {
  return <div className="rail-tip">{children}</div>;
}
function Star() {
  return <span className="rail-star" aria-hidden>★</span>;
}

export const STAGES: Stage[] = [
  // 0. Welcome
  {
    id: 'welcome',
    kind: 'panel',
    label: 'edb',
    badge: 'Welcome',
    title: 'edb · The Ethereum Project Debugger',
    body: <p>Press → to take the tour.</p>,
    color: '#c2410c',
  },

  // 1. Continue (F5)
  {
    id: 'continue',
    kind: 'tour',
    label: 'Continue',
    badge: 'Continue · F5',
    title: 'Run to the next breakpoint',
    body: (
      <>
        <Sec>What it does</Sec>
        <p>
          <code>Continue</code> runs forward until execution reaches the
          next stop, or the end of the trace.
        </p>
        <Sec>Two stops are armed here</Sec>
        <ul>
          <li>
            <strong>Breakpoint</strong> on line <code>234</code>,{' '}
            <code>modifier whenNotPaused()</code>.
          </li>
          <li>
            <strong>Watchpoint</strong> on line <code>242</code>,{' '}
            <code>modifier whenPaused()</code>, guarded by{' '}
            <code>paused == true</code>.
          </li>
        </ul>
        <Sec>Watch the cursor</Sec>
        <ul>
          <li>First click of Continue lands on <code>234</code>.</li>
          <li>Second click lands on <code>242</code>.</li>
        </ul>
      </>
    ),
    color: '#d97706',
    target: { kind: 'selector', selector: '.btn-continue' },
    side: 'bottom',
    anim: 'continue',
  },

  // 2. Step Into (F11): Step Into / Step Over / Step Out explanations
  {
    id: 'step-into',
    kind: 'tour',
    label: 'Step Into',
    badge: 'Stepping · F11',
    title: 'One statement at a time',
    body: (
      <>
        <Sec>Three ways to step</Sec>
        <ul>
          <li>
            <strong>Step Into <code>F11</code></strong> <Star/>{' '}
            <em>recommended.</em> Advance one Solidity statement; if the
            cursor is on a function call, descend into the callee. The
            most reliable stepping mode in EDB.
          </li>
          <li>
            <strong>Step Over <code>F10</code></strong>. Skip past calls
            without entering them, staying at the same call depth.{' '}
            <em>(Best-effort, currently in beta.)</em>
          </li>
          <li>
            <strong>Step Out</strong>. Run forward until the current
            function returns, leaving you one call level shallower.
          </li>
        </ul>
        <Tip>
          When in doubt, use <strong>Step Into</strong>. It walks
          execution exactly the way Solidity does, with no skipping
          and no surprises.
        </Tip>
      </>
    ),
    color: '#7c3aed',
    target: { kind: 'selector', selector: '.btn-step-into, .btn-step-out, .btn-step-over' },
    side: 'bottom',
    anim: 'step-into',
  },

  // 3. Reverse step / Reverse Continue
  {
    id: 'reverse',
    kind: 'tour',
    label: 'Reverse',
    badge: 'Reverse · time travel',
    title: 'Step backward through time',
    body: (
      <>
        <Sec>Two backward controls</Sec>
        <ul>
          <li>
            <strong>Reverse Step</strong>. Undo one snapshot. Click again
            to keep going back, one statement per click.
          </li>
          <li>
            <strong>Reverse Continue</strong>. Rewind to the previous
            breakpoint or watchpoint.
          </li>
        </ul>
        <Sec>Why this is reliable</Sec>
        <p>
          The full snapshot trace is materialised before stepping starts,
          so reversing is just an array index. <strong>No replay drift</strong>,
          no re-execution, no surprises.
        </p>
      </>
    ),
    color: '#0d9488',
    target: { kind: 'selector', selector: '.btn-reverse-step' },
    side: 'bottom',
    anim: 'reverse',
  },

  // 4. Workspace / Activity bar (4 panes)
  {
    id: 'workspace',
    kind: 'tour',
    label: 'Workspace',
    badge: 'Workspace · 4 panes',
    title: 'Explorer · Trace · Variables · Breakpoints',
    body: (
      <>
        <Sec>The activity rail</Sec>
        <p className="mobile-sheet-hide">
          VS Code-style icon column on the left. Four panes, one
          keystroke each.
        </p>
        <ul>
          <li>
            <strong>Explorer</strong>. The file tree. Each contract
            address is a folder; the source files inside open into editor
            tabs.
          </li>
          <li>
            <strong>Trace</strong>. The full call trace for this
            transaction. Click any frame to jump to its first snapshot.
          </li>
          <li>
            <strong>Variables</strong>. Locals and state side-by-side
            with a Watch input. The pane you'll spend the most time in.
          </li>
          <li>
            <strong>Breakpoints</strong>. Plain breakpoints and
            watchpoints, with the conditions you've attached.
          </li>
        </ul>
        <Tip>
          The cursor cycles through each icon so you can see how the
          sidebar swaps content as the active pane changes.
        </Tip>
      </>
    ),
    color: '#2b6cb0',
    target: { kind: 'selector', selector: '.ide-activity' },
    side: 'right',
    anim: 'workspace',
  },

  // 5. Variable Inspection
  {
    id: 'locals',
    kind: 'tour',
    label: 'Variable Inspection',
    badge: 'Variable Inspection',
    title: 'Locals & state, fully decoded',
    body: (
      <>
        <Sec>What you see</Sec>
        <ul>
          <li><strong>Locals</strong> populate live as the function body assigns them.</li>
          <li><strong>State variables</strong> update as storage changes.</li>
          <li><strong>Mappings, arrays, structs</strong> are all decoded, not raw hex.</li>
          <li><strong>Type chips</strong> tag each row with its Solidity type (<code>uint256</code>, <code>address</code>, <code>bool</code>, …).</li>
        </ul>
        <Sec>Why it matters</Sec>
        <ul>
          <li>See exactly what each line is doing, no manual decoding required.</li>
          <li>Track storage changes step by step as the transaction evolves.</li>
          <li>Compare values across snapshots to pinpoint where things went wrong.</li>
          <li>Catch off-by-one bugs and unexpected state mutations the moment they happen.</li>
        </ul>
      </>
    ),
    color: '#2e8b2e',
    target: { kind: 'selector', selector: '.ide-bottom' },
    side: 'top',
    anim: 'locals',
  },

  // 6. Watch / Expression eval
  {
    id: 'watch',
    kind: 'tour',
    label: 'Eval',
    badge: 'Solidity REPL',
    title: 'Evaluate any expression',
    body: (
      <>
        <Sec>What Watch can do</Sec>
        <ul>
          <li>Type <strong>any</strong> Solidity expression: function calls, arithmetic, storage reads, ABI casts.</li>
          <li>Re-evaluated automatically on every snapshot change.</li>
          <li>Promote a watch into a <strong>watchpoint</strong> that fires when its value matches a condition.</li>
        </ul>
        <Sec>Examples</Sec>
        <ul>
          <li><code>balanceOf(msg.sender)</code></li>
          <li><code>_totalSupply &gt; 0</code></li>
          <li><code>paused == true</code></li>
        </ul>
      </>
    ),
    color: '#7c3aed',
    target: { kind: 'selector', selector: '.ide-subtab-watch' },
    side: 'top',
    anim: 'watch',
  },

  // 7. Decompilation (coming soon)
  {
    id: 'decompile-soon',
    kind: 'tour',
    label: 'Decompile',
    badge: 'Coming soon',
    title: 'Bytecode decompilation',
    body: (
      <>
        <Sec>When source is missing</Sec>
        <p>
          The file <code>0x9bf1…a213</code> here is a contract that
          arrived with bytecode. EDB will decompile it into
          readable, fully-steppable pseudo-Solidity, so unverified
          calls debug just like your own code.
        </p>
        <Tip>
          <em>Coming soon</em> in a future release. The placeholder pops
          out next to the bytecode file in the explorer tree.
        </Tip>
      </>
    ),
    color: '#0d9488',
    target: { kind: 'selector', selector: '.ide-bytecode-file' },
    side: 'right',
    anim: 'decompile-soon',
  },

  // 8. Snapshot precision
  {
    id: 'snapshot-precision',
    kind: 'tour',
    label: 'Precision',
    badge: 'Why precision matters',
    title: 'Snapshot N / 34',
    body: (
      <>
        <Sec>The model</Sec>
        <ul>
          <li>Each snapshot = <strong>one executed Solidity statement</strong>.</li>
          <li>The trace is fully materialised before stepping starts.</li>
          <li>Time travel is just an array index.</li>
        </ul>
        <Sec>What you get</Sec>
        <ul>
          <li>
            <strong>Among the deepest observability</strong> of any Solidity
            debugger we know of: every statement is reachable, every value
            is decoded, in either direction.
          </li>
          <li><strong>No replay drift.</strong> Reverse is symmetric with forward.</li>
          <li><strong>Jump anywhere.</strong> Pick a snapshot and inspect; revisit any moment of the transaction.</li>
        </ul>
        <p style={{ marginTop: 8 }}>
          This transaction has <strong>34</strong> snapshots. You can
          revisit any of them.
        </p>
      </>
    ),
    color: '#c2410c',
    target: { kind: 'selector', selector: '.ide-snapshot-counter' },
    side: 'top',
    anim: 'snapshot-cycle',
  },

  // 9. Outro
  {
    id: 'outro',
    kind: 'panel',
    label: 'Why EDB',
    badge: 'Why EDB · Get involved',
    title: 'A debugger that finally matches Solidity',
    body: <p>Other tools show you traces. EDB lets you inhabit the execution.</p>,
    color: '#d4608a',
  },
];

export const FIRST_TOUR_INDEX = STAGES.findIndex((s) => s.kind === 'tour');
export const LAST_TOUR_INDEX = (() => {
  for (let i = STAGES.length - 1; i >= 0; i--) {
    if (STAGES[i]!.kind === 'tour') return i;
  }
  return -1;
})();
