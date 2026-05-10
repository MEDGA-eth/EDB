import { useEffect, useRef, useState } from 'react';
import type { AnimKey } from '../data/stages';

/* ────────────────────────────────────────────────────────────────
   IdeMock. A real-DOM rendering of the EDB IDE in dark mode.
   Per-stage animation scripts mutate React state; CSS ferries the
   visuals (active line, breakpoint pulses, sub-tab cross-fade, …).
   ──────────────────────────────────────────────────────────────── */

type ActivityKind = 'explorer' | 'trace' | 'variables' | 'breakpoints';
type SubTab = 'vars' | 'watch';

const TOTAL_SNAPSHOTS = 34;

/* source under debug. Lines lifted from the dark UI screenshot. */
const MAIN_CODE: { n: number; text: string }[] = [
  { n: 225, text: '    event Pause();' },
  { n: 226, text: '    event Unpause();' },
  { n: 227, text: '' },
  { n: 228, text: '    bool public paused = false;' },
  { n: 229, text: '' },
  { n: 230, text: '    /**' },
  { n: 231, text: '     * @dev Modifier to make a function callable only when not paused.' },
  { n: 232, text: '     */' },
  { n: 233, text: '' },
  { n: 234, text: '    modifier whenNotPaused() {' },
  { n: 235, text: '        require(!paused);' },
  { n: 236, text: '        _;' },
  { n: 237, text: '    }' },
  { n: 238, text: '' },
  { n: 239, text: '    /**' },
  { n: 240, text: '     * @dev Modifier to make a function callable only when paused.' },
  { n: 241, text: '     */' },
  { n: 242, text: '    modifier whenPaused() {' },
  { n: 243, text: '        require(paused);' },
];

/* ── synthetic callee, shown briefly during the Step-Into demo ── */
const CALLEE_CODE: { n: number; text: string }[] = [
  { n: 412, text: '    /// internal helper used by `whenNotPaused`' },
  { n: 413, text: '    function _checkPaused(bool flag) internal pure {' },
  { n: 414, text: '        if (flag) {' },
  { n: 415, text: '            revert("paused");' },
  { n: 416, text: '        }' },
  { n: 417, text: '    }' },
  { n: 418, text: '' },
];

/* "is this line a real code statement?" Skips blanks plus Solidity
   doc/line comments. Used by all stepping animations: per the EDB model,
   we step at executable Solidity statements, never at comments. */
function isCodeLine(text: string): boolean {
  const t = text.trim();
  if (t === '') return false;
  if (t.startsWith('//') || t.startsWith('/*') || t.startsWith('*') || t.startsWith('///')) return false;
  return true;
}

/* ── tokenizer ──────────────────────────────────────────────────── */

const KEYWORDS = new Set([
  'function', 'modifier', 'require', 'bool', 'public', 'private', 'internal', 'external',
  'event', 'if', 'else', 'revert', 'pure', 'view', 'returns', 'return',
  'true', 'false', 'contract', 'mapping', 'storage', 'memory',
]);
const TYPES = new Set([
  'address', 'uint', 'uint8', 'uint16', 'uint32', 'uint64', 'uint128', 'uint256',
  'int', 'int256', 'bytes', 'bytes32', 'string',
]);

function highlight(line: string): React.ReactNode[] {
  const trimmed = line.trimStart();
  // Whole-line comment
  if (
    trimmed.startsWith('//') ||
    trimmed.startsWith('/*') ||
    trimmed.startsWith('*') ||
    trimmed.startsWith('///')
  ) {
    return [
      <span key="cmt" className="tok-cmt">
        {line}
      </span>,
    ];
  }
  const out: React.ReactNode[] = [];
  // Exclude `"` from the punctuation alternative so `revert("paused")` doesn't
  // get tokenised as `revert` + `("` (greedy punct swallowing the quote) +
  // `paused` (identifier, no string colour) + `");` (more punct).
  const re = /("[^"]*"|\d+|[a-zA-Z_][a-zA-Z0-9_]*|[^\w\s"]+|\s+)/g;
  let m: RegExpExecArray | null;
  let key = 0;
  while ((m = re.exec(line)) !== null) {
    const t = m[0];
    if (/^\s+$/.test(t)) {
      out.push(t);
    } else if (t.startsWith('"')) {
      out.push(
        <span key={key++} className="tok-str">
          {t}
        </span>,
      );
    } else if (/^\d+$/.test(t)) {
      out.push(
        <span key={key++} className="tok-num">
          {t}
        </span>,
      );
    } else if (KEYWORDS.has(t)) {
      out.push(
        <span key={key++} className="tok-kw">
          {t}
        </span>,
      );
    } else if (TYPES.has(t)) {
      out.push(
        <span key={key++} className="tok-type">
          {t}
        </span>,
      );
    } else if (/^[a-zA-Z_]/.test(t)) {
      out.push(
        <span key={key++} className="tok-id">
          {t}
        </span>,
      );
    } else {
      out.push(
        <span key={key++} className="tok-punct">
          {t}
        </span>,
      );
    }
  }
  return out;
}

/* ── small helpers for animation choreography ───────────────────── */

function delay(ms: number): Promise<void> {
  return new Promise((res) => setTimeout(res, ms));
}

/* ── activity rail icons ────────────────────────────────────────── */

function FolderIcon() {
  return (
    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
    </svg>
  );
}
function NetworkIcon() {
  return (
    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <rect x="9" y="2" width="6" height="6" rx="1" />
      <rect x="2" y="16" width="6" height="6" rx="1" />
      <rect x="16" y="16" width="6" height="6" rx="1" />
      <path d="M5 16v-2a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2v2M12 8v4" />
    </svg>
  );
}
function EyeIcon() {
  return (
    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M2 12s3-7 10-7 10 7 10 7-3 7-10 7-10-7-10-7z" />
      <circle cx="12" cy="12" r="3" />
    </svg>
  );
}
function BreakpointGlyph() {
  return (
    <svg width="24" height="24" viewBox="0 0 24 24" fill="none">
      <circle cx="12" cy="12" r="9" stroke="currentColor" strokeWidth="1.6" opacity="0.5" />
      <circle cx="12" cy="12" r="5" fill="#f87171" />
    </svg>
  );
}

const ACTIVITIES: { key: ActivityKind; label: string; tint: string; Icon: () => React.ReactElement }[] = [
  { key: 'explorer', label: 'Explorer', tint: '#7cb8f5', Icon: FolderIcon },
  { key: 'trace', label: 'Trace', tint: '#c4b5fd', Icon: NetworkIcon },
  { key: 'variables', label: 'Variables', tint: '#5eead4', Icon: EyeIcon },
  { key: 'breakpoints', label: 'Breakpoints', tint: '#f87171', Icon: BreakpointGlyph },
];

/* ── toolbar buttons (matches DebugToolbar.tsx order) ──────────── */

const TOOLBAR: { id: string; label: string; kbd?: string; cls?: string }[] = [
  { id: 'continue',         label: 'Continue',         kbd: 'F5',  cls: 'btn-continue' },
  { id: 'step-into',        label: 'Step Into',        kbd: 'F11', cls: 'btn-step-into' },
  { id: 'step-out',         label: 'Step Out',         kbd: '⇧F11', cls: 'btn-step-out' },
  { id: 'step-over',        label: 'Step Over',        kbd: 'F10', cls: 'btn-step-over' },
  { id: 'restart',          label: 'Restart',          kbd: '⇧⌘F5', cls: 'btn-restart' },
  { id: 'stop',             label: 'Stop',             kbd: '⇧F5', cls: 'btn-stop' },
  { id: 'reverse-continue', label: 'Reverse Continue', cls: 'btn-reverse-continue' },
  { id: 'reverse-step',     label: 'Reverse Step',     cls: 'btn-reverse-step' },
  { id: 'prev-call',        label: 'Prev Call',        cls: 'btn-prev-call' },
  { id: 'next-call',        label: 'Next Call',        cls: 'btn-next-call' },
];

/* ── component ─────────────────────────────────────────────────── */

export type IdeMockProps = {
  /** stage id (drives data-stage), used for static styling like decompile badge */
  stage: string;
  /** which animation script to run while this stage is active */
  anim: AnimKey;
  /** if true, the mock is dimmed/inactive (e.g. on hero/strengths/cta stages) */
  dim?: boolean;
};

const DEFAULT_ACTIVE_LINE = 234;
const DEFAULT_SNAPSHOT = 1;

export default function IdeMock({ stage, anim, dim }: IdeMockProps) {
  const [snapshot, setSnapshot] = useState<number>(DEFAULT_SNAPSHOT);
  const [activeLine, setActiveLine] = useState<number>(DEFAULT_ACTIVE_LINE);
  const [activity, setActivity] = useState<ActivityKind>('explorer');
  const [subTab, setSubTab] = useState<SubTab>('vars');
  const [showCallee, setShowCallee] = useState(false);
  // Both breakpoints (plain + watchpoint) are always rendered in the code so
  // every stepping demo has them visible.
  const [decompileSoon, setDecompileSoon] = useState(false);
  const [localsRevealed, setLocalsRevealed] = useState(3);
  const [watchRevealed, setWatchRevealed] = useState(0);
  // Mock mouse cursor + click ripple; null = hidden.
  const [cursor, setCursor] = useState<{ x: number; y: number; click: boolean } | null>(null);
  // "Paused on a breakpoint" overlay for the active line.
  const [stoppedAtBp, setStoppedAtBp] = useState(false);
  // What the user is currently typing into the Watch input.
  const [watchTyping, setWatchTyping] = useState<string>('');

  const ideRootRef = useRef<HTMLDivElement>(null);

  /* Keep the active code line in view. Mostly relevant on mobile, where
     the side-by-side layout makes the code column shorter; harmless on
     desktop because `block: 'nearest'` is a no-op when the line is
     already visible. */
  useEffect(() => {
    const root = ideRootRef.current;
    if (!root) return;
    const el = root.querySelector<HTMLElement>(`.ide-code [data-line="${activeLine}"]`);
    if (!el) return;
    el.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'smooth' });
  }, [activeLine]);

  /* ── per-stage animation choreography ── */
  useEffect(() => {
    let cancelled = false;
    let timers: ReturnType<typeof setTimeout>[] = [];
    const wait = (ms: number) =>
      new Promise<void>((res) => {
        const t = setTimeout(() => res(), ms);
        timers.push(t);
      });

    /* default visual state; each branch tweaks from here */
    setSnapshot(DEFAULT_SNAPSHOT);
    setActiveLine(DEFAULT_ACTIVE_LINE);
    setActivity('explorer');
    setSubTab('vars');
    setShowCallee(false);
    setDecompileSoon(false);
    setLocalsRevealed(3);
    setWatchRevealed(0);
    setCursor(null);
    setStoppedAtBp(false);
    setWatchTyping('');

    const lineAt = (idx: number) => MAIN_CODE[idx]?.n ?? DEFAULT_ACTIVE_LINE;
    const findIdx = (n: number) => MAIN_CODE.findIndex((l) => l.n === n);

    /* Mock-mouse helpers: locate a target inside the IDE mock and either
       glide the cursor toward it (smooth CSS transition) or snap-click. */
    async function moveCursorTo(selector: string, ms = 520) {
      const root = ideRootRef.current;
      if (!root) return;
      const el = root.querySelector(selector) as HTMLElement | null;
      if (!el) return;
      const r = el.getBoundingClientRect();
      const rootR = root.getBoundingClientRect();
      const x = r.left + r.width * 0.5 - rootR.left;
      const y = r.top + r.height * 0.55 - rootR.top;
      setCursor((prev) => (prev ? { ...prev, x, y } : { x, y, click: false }));
      await wait(ms);
    }
    async function clickCursor(holdMs = 380) {
      setCursor((c) => (c ? { ...c, click: true } : c));
      await wait(holdMs);
      setCursor((c) => (c ? { ...c, click: false } : c));
      // brief settle before the click's effect kicks in
      await wait(140);
    }

    /* runForwardUntil: advance the active line + snapshot (skipping blanks)
       until the supplied line number is hit. Used by Continue / Reverse. */
    async function runForwardUntil(stopLine: number, startIdx: number, startSnap: number, perStepMs = 170) {
      let i = startIdx;
      let s = startSnap;
      while (!cancelled) {
        do { i = Math.min(i + 1, MAIN_CODE.length - 1); } while (i < MAIN_CODE.length - 1 && !isCodeLine(MAIN_CODE[i]!.text));
        s += 1;
        setSnapshot(s);
        setActiveLine(MAIN_CODE[i]!.n);
        if (MAIN_CODE[i]!.n === stopLine) return { idx: i, snap: s };
        await wait(perStepMs);
      }
      return { idx: i, snap: s };
    }

    async function continueAnim() {
      // Open the Breakpoints sidebar; viewers see the two stops up front.
      setActivity('breakpoints');
      while (!cancelled) {
        // start above the first breakpoint, no stops yet
        setSnapshot(1);
        setActiveLine(228);                            // bool public paused = false;
        setStoppedAtBp(false);
        await wait(900);

        // user clicks Continue: run forward until the plain breakpoint at 234
        await moveCursorTo('.btn-continue', 620);
        await clickCursor(220);
        const startA = findIdx(228);
        const after1 = await runForwardUntil(234, startA, 1, 170);
        setStoppedAtBp(true);
        await wait(2000);                              // dwell on the bp

        // user clicks Continue again: run to the watchpoint at 240
        setStoppedAtBp(false);
        await moveCursorTo('.btn-continue', 380);
        await clickCursor(220);
        await runForwardUntil(242, after1.idx, after1.snap, 160);
        setStoppedAtBp(true);
        await wait(2400);
      }
    }

    async function stepIntoAnim() {
      setActivity('breakpoints');
      while (!cancelled) {
        setShowCallee(false);
        setSnapshot(1);
        setActiveLine(235);                            // require(!paused);
        setStoppedAtBp(false);
        await wait(900);

        // user clicks Step Into → enter the callee
        await moveCursorTo('.btn-step-into', 560);
        await clickCursor(220);
        setShowCallee(true);
        setSnapshot(2);
        setActiveLine(413);                            // function _checkPaused
        await wait(900);
        // step over a couple of times
        await moveCursorTo('.btn-step-over', 380);
        await clickCursor(180);
        setSnapshot(3);
        setActiveLine(414);
        await wait(700);
        await clickCursor(180);
        setSnapshot(4);
        setActiveLine(415);
        await wait(700);

        // user clicks Step Out → return to caller
        await moveCursorTo('.btn-step-out', 380);
        await clickCursor(220);
        setShowCallee(false);
        setSnapshot(5);
        setActiveLine(236);
        await wait(2000);
      }
    }

    async function reverseAnim() {
      setActivity('breakpoints');
      while (!cancelled) {
        // Forward run, recording every visited (snapshot, line) pair so the
        // reverse phase can replay the path step by step. Without recording,
        // the reverse loop would keep advancing `i` forward (the bug the
        // user spotted: clicking Reverse Step appeared to step DOWN).
        const path: { snap: number; line: number }[] = [];
        let i = findIdx(DEFAULT_ACTIVE_LINE);
        let s = 1;
        setSnapshot(s);
        setActiveLine(MAIN_CODE[i]!.n);
        setStoppedAtBp(false);
        path.push({ snap: s, line: MAIN_CODE[i]!.n });
        await wait(700);
        for (let k = 0; k < 5 && !cancelled; k++) {
          do { i = Math.min(i + 1, MAIN_CODE.length - 1); } while (i < MAIN_CODE.length - 1 && !isCodeLine(MAIN_CODE[i]!.text));
          s += 1;
          setSnapshot(s);
          setActiveLine(MAIN_CODE[i]!.n);
          path.push({ snap: s, line: MAIN_CODE[i]!.n });
          await wait(280);
        }
        await wait(900);

        // User clicks Reverse Step repeatedly: each click moves one entry
        // back through the recorded path, so the line goes UP and the
        // snapshot counter ticks DOWN, exactly one step per click.
        for (let k = path.length - 2; k >= 0 && !cancelled; k--) {
          await moveCursorTo('.btn-reverse-step', 360);
          await clickCursor();
          const step = path[k]!;
          setSnapshot(step.snap);
          setActiveLine(step.line);
          await wait(450);
        }
        await wait(1700);
      }
    }

    async function workspaceAnim() {
      const seq: ActivityKind[] = ['explorer', 'trace', 'variables', 'breakpoints'];
      let i = 0;
      while (!cancelled) {
        const key = seq[i % seq.length]!;
        await moveCursorTo(`.ide-activity-btn[data-key="${key}"]`, 520);
        await clickCursor(180);
        setActivity(key);
        await wait(1100);
        i++;
      }
    }

    async function localsAnim() {
      // Switch the rail icon to Variables so the side panel fits the story.
      setActivity('variables');
      while (!cancelled) {
        setLocalsRevealed(0);
        await wait(420);
        for (let i = 1; i <= 3 && !cancelled; i++) {
          setLocalsRevealed(i);
          await wait(560);
        }
        await wait(2400);
      }
    }

    async function watchAnim() {
      setActivity('variables');
      setSubTab('vars');
      setWatchTyping('');
      setWatchRevealed(0);
      await wait(620);
      const exprs = ['_totalSupply > 0', 'basisPointsRate * 100', 'paused'];
      while (!cancelled) {
        // user clicks the Watch sub-tab
        await moveCursorTo('.ide-subtab-watch', 540);
        await clickCursor();
        setSubTab('watch');
        setWatchRevealed(0);
        setWatchTyping('');
        await wait(540);

        for (let i = 0; i < exprs.length && !cancelled; i++) {
          // user clicks the input cell
          await moveCursorTo('.ide-watch-input-cell', 380);
          await clickCursor(260);
          // type the expression character by character
          const expr = exprs[i]!;
          setWatchTyping('');
          for (let k = 1; k <= expr.length && !cancelled; k++) {
            setWatchTyping(expr.slice(0, k));
            await wait(60);
          }
          await wait(600);
          // commit: row flips in, typed text clears
          setWatchRevealed(i + 1);
          setWatchTyping('');
          await wait(700);
        }
        await wait(2400);
      }
    }

    async function decompileSoonAnim() {
      setActivity('explorer');
      await wait(420);
      await moveCursorTo('.ide-bytecode-file', 560);
      await wait(420);
      setDecompileSoon(true);
      await wait(2600);
    }

    async function snapshotCycleAnim() {
      setActivity('breakpoints');
      // jump-around to dramatise time travel
      const targets = [1, 6, 17, 28, 34, 24, 12, 4];
      while (!cancelled) {
        for (const t of targets) {
          if (cancelled) return;
          setSnapshot(t);
          // pick a line scaled by snapshot to show motion
          const idx = Math.min(
            MAIN_CODE.length - 1,
            Math.max(0, Math.round((t / TOTAL_SNAPSHOTS) * MAIN_CODE.length)),
          );
          setActiveLine(lineAt(idx));
          await wait(420);
        }
      }
    }

    switch (anim) {
      case 'continue': continueAnim(); break;
      case 'step-into': stepIntoAnim(); break;
      case 'reverse': reverseAnim(); break;
      case 'workspace': workspaceAnim(); break;
      case 'locals': localsAnim(); break;
      case 'watch': watchAnim(); break;
      case 'decompile-soon': decompileSoonAnim(); break;
      case 'snapshot-cycle': snapshotCycleAnim(); break;
      case 'idle':
      default:
        /* idle: leave defaults */
        break;
    }

    return () => {
      cancelled = true;
      for (const t of timers) clearTimeout(t);
      timers = [];
    };
  }, [anim]);

  const visibleCode = showCallee ? CALLEE_CODE : MAIN_CODE;

  return (
    <div
      ref={ideRootRef}
      className={`ide-mock ${dim ? 'is-dim' : ''}`}
      data-stage={stage}
      aria-hidden={dim ? true : undefined}
    >
      {/* top toolbar ────────────────────────────────────────────── */}
      <div className="ide-toolbar">
        <div className="ide-debug-chip">DEBUG</div>
        {TOOLBAR.map((b, i) => (
          <button
            key={b.id}
            type="button"
            className={`ide-tb-btn ${b.cls ?? ''} ${
              b.id.startsWith('reverse-') ? 'btn-reverse-group' : ''
            }`}
            tabIndex={-1}
          >
            <span className="ide-tb-icon" aria-hidden>
              {iconFor(b.id)}
            </span>
            <span className="ide-tb-label">{b.label}</span>
            {b.kbd && <kbd className="ide-tb-kbd">{b.kbd}</kbd>}
            {/* invisible separator group bracket. Reverse Continue + Reverse Step share a hover ring. */}
            {(b.id === 'reverse-step') && <span className="ide-tb-group-tail" aria-hidden />}
            {(b.id === 'reverse-continue') && <span className="ide-tb-group-head" aria-hidden />}
            {i === 5 && <span className="ide-tb-divider" aria-hidden />}
          </button>
        ))}
      </div>

      {/* body grid ───────────────────────────────────────────────── */}
      <div className="ide-body">
        {/* activity rail */}
        <div className="ide-activity">
          {ACTIVITIES.map((a) => (
            <button
              key={a.key}
              type="button"
              className={`ide-activity-btn ${activity === a.key ? 'is-active' : ''}`}
              style={{ ['--tint' as string]: a.tint } as React.CSSProperties}
              tabIndex={-1}
              data-key={a.key}
            >
              <span className="ide-activity-rail" aria-hidden />
              <a.Icon />
              <span className="ide-activity-label">{a.label}</span>
            </button>
          ))}
        </div>

        {/* sidebar (content depends on the current activity) */}
        <aside className="ide-sidebar">
          {activity === 'breakpoints' ? (
            <BreakpointsSideView />
          ) : activity === 'trace' ? (
            <TraceSideView />
          ) : activity === 'variables' ? (
            <VariablesSideView />
          ) : (
            <ExplorerSideView decompileSoon={decompileSoon} />
          )}
        </aside>

        {/* main work area */}
        <section className="ide-main">
          {/* file-tab strip */}
          <div className="ide-tabs">
            <div className="ide-tab is-active">
              Contract<span className="ide-tab-x">×</span>
            </div>
          </div>
          {/* editor toolbar */}
          <div className="ide-edtoolbar">
            <span className="ide-edt-btn">⌕ Find</span>
            <span className="ide-edt-btn">⎘ Copy</span>
            <span className="ide-edt-btn">↩ Wrap</span>
            <span className="ide-edt-btn is-on">▤ Line numbers</span>
            <span className="ide-edt-btn">● Breakpoint here</span>
            <span className="ide-edt-btn">⤴ Reveal current</span>
            <span className="ide-edt-spacer" />
            <span className="ide-edt-meta">0xdac1…1ec7 / Contract</span>
          </div>
          {/* code */}
          <div className="ide-code">
            {visibleCode.map((l) => {
              const isActive = l.n === activeLine;
              const isPlainBp = l.n === 234;
              const isCondBp = l.n === 242;
              const isStopped = isActive && stoppedAtBp && (isPlainBp || isCondBp);
              return (
                <div
                  key={l.n}
                  className={`ide-code-line ${isActive ? 'is-active' : ''} ${isStopped ? 'is-stopped' : ''}`}
                  data-line={l.n}
                >
                  <span className="ide-gutter">
                    {isPlainBp && <span className="ide-bp ide-bp-plain" title="breakpoint" />}
                    {isCondBp && <span className="ide-bp ide-bp-watch" title="watchpoint (breakpoint with condition)" />}
                  </span>
                  <span className="ide-line-num">{l.n}</span>
                  <span className="ide-line-text">
                    {highlight(l.text)}
                    {isActive && <span className="ide-caret" aria-hidden />}
                  </span>
                  {/* The watchpoint's condition pill stays mounted; it just
                      blurs out when the line transitions into the "hit"
                      state, which then blurs in over the same slot. */}
                  {isCondBp && (
                    <span
                      className={`ide-cond-pill ${isStopped ? 'is-fading' : ''}`}
                      title="Watchpoint condition"
                    >
                      <span className="ide-cond-when">when</span>
                      <code>paused == true</code>
                    </span>
                  )}
                  {isStopped && (
                    <span className={`ide-stop-tag ${isCondBp ? 'is-watch' : 'is-plain'}`}>
                      ⏸ {isCondBp ? 'watchpoint hit' : 'breakpoint hit'}
                    </span>
                  )}
                </div>
              );
            })}
          </div>

          {/* bottom panel */}
          <div className="ide-bottom">
            <div className="ide-tabs">
              <div className="ide-tab is-active">Display<span className="ide-tab-x">×</span></div>
              <div className="ide-tab">Terminal</div>
            </div>
            <div className="ide-edtoolbar">
              <span className="ide-edt-btn">↻ Refresh</span>
              <span className="ide-edt-btn">⎘ Copy</span>
              <span className="ide-edt-meta">snapshot {snapshot}</span>
            </div>
            <div className="ide-subtabs">
              {(
                ['Variables', 'Watch', 'Stack', 'Memory', 'Storage', 'Transient', 'Calldata', 'Output'] as const
              ).map((t) => {
                const key = t.toLowerCase();
                const active = (subTab === 'watch' && key === 'watch') || (subTab === 'vars' && key === 'variables');
                return (
                  <div
                    key={t}
                    className={`ide-subtab ${active ? 'is-active' : ''} ide-subtab-${key}`}
                  >
                    {t}
                  </div>
                );
              })}
            </div>
            <div className="ide-bottom-body">
              {subTab === 'vars' ? (
                <VariablesPane revealed={localsRevealed} />
              ) : (
                <WatchPane revealed={watchRevealed} typing={watchTyping} />
              )}
            </div>
          </div>
        </section>
      </div>

      {/* status bar ──────────────────────────────────────────────── */}
      <div className="ide-status">
        <span className="ide-status-mark">edb</span>
        <span className="ide-snapshot-counter" data-cycling={anim === 'snapshot-cycle' ? 'true' : 'false'}>
          snapshot {snapshot} / {TOTAL_SNAPSHOTS}
        </span>
        <span className="ide-status-spacer" />
        <span className="ide-status-pill is-ok">● Connected</span>
        <span className="ide-status-pill">🌙 Dark</span>
        <span className="ide-status-pill">? Help</span>
      </div>

      {/* "Decompiler coming soon" callout, anchored to the bytecode file row.
          Rendered at the IDE root so it can pop out of the sidebar's overflow. */}
      {decompileSoon && (
        <div className="ide-decompile-callout" role="status" aria-live="polite">
          <span className="ide-decompile-arrow" aria-hidden />
          <div className="ide-decompile-card">
            <div className="ide-decompile-badge">
              <span className="sparkle" aria-hidden>✨</span>
              <span>Decompiler · coming soon</span>
            </div>
            <div className="ide-decompile-title">Bytecode &rarr; Solidity</div>
            <div className="ide-decompile-text">
              EDB will lift bytecode contracts into readable, fully-steppable
              pseudo-Solidity, so unverified calls debug just like your own.
            </div>
          </div>
        </div>
      )}

      {/* mock mouse cursor (driven by per-stage animations) */}
      {cursor && (
        <span
          className={`ide-cursor ${cursor.click ? 'is-clicking' : ''}`}
          style={{ left: cursor.x, top: cursor.y }}
          aria-hidden
        >
          <CursorArrow />
        </span>
      )}
    </div>
  );
}

/* Mac-style left-arrow cursor used by the mock mouse animation. */
function CursorArrow() {
  return (
    <svg width="22" height="24" viewBox="0 0 22 24" aria-hidden>
      <path
        d="M3 2 L3 20 L8 16 L11 22 L14 21 L11 15 L18 14 Z"
        fill="#ffffff"
        stroke="#1a1207"
        strokeWidth="1.5"
        strokeLinejoin="round"
      />
    </svg>
  );
}

/* ── icons inside toolbar buttons ───────────────────────────────── */

function iconFor(id: string): React.ReactNode {
  switch (id) {
    case 'continue':
      return (
        <svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor"><path d="M3 2 L13 8 L3 14 Z" /></svg>
      );
    case 'step-into':
      return (
        <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2">
          <path d="M8 2 v8 M5 7 l3 3 3-3" strokeLinecap="round" strokeLinejoin="round" />
          <circle cx="8" cy="13" r="1.4" fill="currentColor" stroke="none" />
        </svg>
      );
    case 'step-out':
      return (
        <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2">
          <path d="M8 14 v-8 M5 9 l3 -3 3 3" strokeLinecap="round" strokeLinejoin="round" />
          <circle cx="8" cy="3" r="1.4" fill="currentColor" stroke="none" />
        </svg>
      );
    case 'step-over':
      return (
        <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2">
          <path d="M2 9 a4 4 0 0 1 12 0" strokeLinecap="round" />
          <path d="M14 9 l-2 -2 M14 9 l2 -2" strokeLinecap="round" strokeLinejoin="round" transform="translate(-2 1)" />
          <circle cx="8" cy="13" r="1.4" fill="currentColor" stroke="none" />
        </svg>
      );
    case 'restart':
      return (
        <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M3 8 a5 5 0 1 1 1.5 3.5" />
          <path d="M2 4 v3 h3" />
        </svg>
      );
    case 'stop':
      return <svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor"><rect x="3" y="3" width="10" height="10" rx="1.5" /></svg>;
    case 'reverse-continue':
      return <svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor"><path d="M13 2 L3 8 L13 14 Z" /></svg>;
    case 'reverse-step':
      return (
        <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M14 4 a5 5 0 1 0 0 8" />
          <path d="M14 12 l-2 -2 M14 12 l-2 2" />
        </svg>
      );
    case 'prev-call':
      return <svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor"><path d="M11 2 L4 8 L11 14 V10 L13 10 V6 L11 6 Z" /></svg>;
    case 'next-call':
      return <svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor"><path d="M5 2 L12 8 L5 14 V10 L3 10 V6 L5 6 Z" /></svg>;
    default:
      return null;
  }
}

/* ── inner panes ────────────────────────────────────────────────── */

function VariablesPane({ revealed }: { revealed: number }) {
  const localsLabel = (
    <div className="ide-pane-section ide-pane-locals">
      <div className="ide-pane-title">LOCALS</div>
      <div className="ide-pane-body">
        <div className="ide-pane-empty">
          No locals here yet. Step forward (<kbd>F11</kbd>) and they'll
          populate as the function body runs.
        </div>
      </div>
    </div>
  );

  const stateRows = [
    { name: '_totalSupply', val: '0x158de5b5c6dc4b0', type: 'uint256', color: '#7cb8f5' },
    { name: 'basisPointsRate', val: '0x0', type: 'uint256', color: '#fbbf24' },
    { name: 'maximumFee', val: '0x0', type: 'uint256', color: '#5eead4' },
  ];
  return (
    <div className="ide-vars">
      {localsLabel}
      <div className="ide-pane-section">
        <div className="ide-pane-title">STATE VARIABLES</div>
        <div className="ide-pane-body">
          {stateRows.slice(0, revealed).map((r, i) => (
            <div key={r.name} className="ide-var-row" style={{ animationDelay: `${i * 80}ms` }}>
              <span className="ide-var-name">{r.name}</span>
              <span className="ide-var-val">{r.val}</span>
              <span className="ide-var-type" style={{ color: r.color }}>{r.type}</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

function WatchPane({ revealed, typing }: { revealed: number; typing: string }) {
  const rows = [
    { expr: '_totalSupply > 0', val: 'true', color: '#86efac' },
    { expr: 'basisPointsRate * 100', val: '0', color: '#fbbf24' },
    { expr: 'paused', val: 'false', color: '#fda4af' },
  ];
  return (
    <div className="ide-vars">
      <div className="ide-pane-section">
        <div className="ide-pane-title">WATCH</div>
        <div className="ide-pane-body">
          {rows.slice(0, revealed).map((r, i) => (
            <div key={r.expr} className="ide-watch-row" style={{ animationDelay: `${i * 80}ms` }}>
              <code className="ide-watch-expr">{r.expr}</code>
              <span className="ide-watch-arrow">→</span>
              <span className="ide-watch-val" style={{ color: r.color }}>{r.val}</span>
            </div>
          ))}
          {revealed < rows.length && (
            <div className="ide-watch-row is-input ide-watch-input-cell">
              {typing ? (
                <code className="ide-watch-expr">
                  {typing}
                  <span className="ide-watch-input-caret" aria-hidden />
                </code>
              ) : (
                <code className="ide-watch-expr placeholder">type any Solidity expression…</code>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

/* sidebar views, picked by the active activity-rail key */

function ExplorerSideView({ decompileSoon }: { decompileSoon: boolean }) {
  return (
    <>
      <div className="ide-sidebar-header">EXPLORER</div>
      <div className="ide-tree">
        {/* verified contract: source available, tree expanded */}
        <div className="ide-tree-row ide-tree-folder is-open">
          <span className="ide-tree-twist">▾</span>
          <span className="ide-tree-mono">0xdac1…1ec7</span>
          <span className="ide-tree-tag is-verified">verified</span>
        </div>
        <div className="ide-tree-row ide-tree-file is-active">
          <span className="ide-tree-leaf-spacer" />
          <span className="ide-tree-leaf">📄</span>
          <span>Contract</span>
        </div>
        {/* unverified contract: source missing. The "coming soon" callout is
            rendered separately at the IDE root so it can pop out as a real
            tooltip-style card rather than crammed into the tree row. */}
        <div className="ide-tree-row ide-tree-folder is-open">
          <span className="ide-tree-twist">▾</span>
          <span className="ide-bytecode-file">
            <span className="ide-tree-mono">0x9bf1…a213</span>
            <span className="ide-tree-tag is-bytecode">bytecode</span>
          </span>
        </div>
        <div className="ide-tree-row ide-tree-file">
          <span className="ide-tree-leaf-spacer" />
          <span className="ide-tree-leaf">📄</span>
          <span style={{ fontStyle: 'italic', opacity: 0.55 }}>(no source)</span>
        </div>
      </div>
    </>
  );
}

function BreakpointsSideView() {
  return (
    <>
      <div className="ide-sidebar-header ide-sidebar-header-row">
        <span>BREAKPOINTS</span>
        <span className="ide-sidebar-count">2</span>
      </div>
      <div className="ide-bp-list">
        <div className="ide-bp-item">
          <span className="ide-bp ide-bp-plain" aria-hidden />
          <div className="ide-bp-meta">
            <div className="ide-bp-loc">
              <span className="ide-bp-file">Contract</span>
              <span className="ide-bp-sep">:</span>
              <span className="ide-bp-line">234</span>
            </div>
            <div className="ide-bp-snippet">
              <span className="tok-kw">modifier</span>{' '}
              <span className="tok-id">whenNotPaused</span>
              <span className="tok-punct">() {'{'}</span>
            </div>
            <div className="ide-bp-tags">
              <span className="ide-bp-tag is-plain">breakpoint</span>
            </div>
          </div>
        </div>
        <div className="ide-bp-item">
          <span className="ide-bp ide-bp-watch" aria-hidden />
          <div className="ide-bp-meta">
            <div className="ide-bp-loc">
              <span className="ide-bp-file">Contract</span>
              <span className="ide-bp-sep">:</span>
              <span className="ide-bp-line">242</span>
            </div>
            <div className="ide-bp-snippet">
              <span className="tok-kw">modifier</span>{' '}
              <span className="tok-id">whenPaused</span>
              <span className="tok-punct">() {'{'}</span>
            </div>
            <div className="ide-bp-tags">
              <span className="ide-bp-tag is-watch">watchpoint</span>
              <code className="ide-bp-cond">
                <span className="ide-bp-cond-key">when</span> paused == true
              </code>
            </div>
          </div>
        </div>
      </div>
    </>
  );
}

function TraceSideView() {
  const FRAMES = [
    { addr: '0xdac1…1ec7', call: 'transferFrom(from, to, 1e18)', depth: 0 },
    { addr: '0xdac1…1ec7', call: 'whenNotPaused()',                depth: 1 },
    { addr: '0xdac1…1ec7', call: '_transfer(from, to, value)',     depth: 1 },
    { addr: '0xdac1…1ec7', call: '_balances[from] -= value',       depth: 2 },
  ];
  return (
    <>
      <div className="ide-sidebar-header">TRACE</div>
      <div className="ide-trace-list">
        {FRAMES.map((f, i) => (
          <div
            key={i}
            className={`ide-trace-frame ${i === 1 ? 'is-active' : ''}`}
            style={{ paddingLeft: 12 + f.depth * 14 }}
          >
            <span className="ide-trace-addr">{f.addr}</span>
            <span className="ide-trace-call">{f.call}</span>
          </div>
        ))}
      </div>
    </>
  );
}

function VariablesSideView() {
  // Mirrors EDB's real VariablesAndWatchSidebar + VarsView card layout: each
  // variable / watch row is its own bordered card with a type chip on the
  // right and the value rendered in mono below the name.
  return (
    <>
      <div className="ide-sidebar-header">VARIABLES &amp; WATCH</div>
      <div className="ide-sb-scroll">
        <div className="ide-sb-section">
          <h3 className="ide-sb-section-head">Locals</h3>
          <div className="ide-sb-empty">No locals yet. Step <kbd>F11</kbd>.</div>
        </div>
        <div className="ide-sb-section">
          <h3 className="ide-sb-section-head">State Variables</h3>
          <ul className="ide-sb-list">
            <li className="ide-sb-card">
              <div className="ide-sb-card-head">
                <span className="ide-sb-card-name">_totalSupply</span>
                <span className="ide-sb-type-chip" style={{ color: '#7cb8f5' }}>uint256</span>
              </div>
              <div className="ide-sb-card-val">0x158de5b5c6dc4b0</div>
            </li>
            <li className="ide-sb-card">
              <div className="ide-sb-card-head">
                <span className="ide-sb-card-name">basisPointsRate</span>
                <span className="ide-sb-type-chip" style={{ color: '#7cb8f5' }}>uint256</span>
              </div>
              <div className="ide-sb-card-val">0x0</div>
            </li>
            <li className="ide-sb-card">
              <div className="ide-sb-card-head">
                <span className="ide-sb-card-name">paused</span>
                <span className="ide-sb-type-chip" style={{ color: '#fbbf24' }}>bool</span>
              </div>
              <div className="ide-sb-card-val">false</div>
            </li>
          </ul>
        </div>
        <div className="ide-sb-section">
          <h3 className="ide-sb-section-head">Watch</h3>
          <ul className="ide-sb-list">
            <li className="ide-sb-card">
              <div className="ide-sb-card-head">
                <code className="ide-sb-card-expr">_totalSupply &gt; 0</code>
                <span className="ide-sb-type-chip" style={{ color: '#fbbf24' }}>bool</span>
              </div>
              <div className="ide-sb-card-val" style={{ color: '#86efac' }}>true</div>
            </li>
            <li className="ide-sb-card">
              <div className="ide-sb-card-head">
                <code className="ide-sb-card-expr">paused</code>
                <span className="ide-sb-type-chip" style={{ color: '#fbbf24' }}>bool</span>
              </div>
              <div className="ide-sb-card-val" style={{ color: '#fda4af' }}>false</div>
            </li>
          </ul>
          <form className="ide-sb-watch-input" onSubmit={(e) => e.preventDefault()}>
            <span className="ide-sb-watch-plus" aria-hidden>+</span>
            <input
              className="ide-sb-watch-field"
              placeholder="e.g. balanceOf(msg.sender)"
              readOnly
              tabIndex={-1}
            />
            <span className="ide-sb-watch-add">Add</span>
          </form>
        </div>
      </div>
    </>
  );
}
