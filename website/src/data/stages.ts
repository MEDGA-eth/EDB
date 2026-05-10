/**
 * 12 stages, modeled on tiny-dec's stage list. The first stage is the hero,
 * the last two are summary/CTA panels; everything in between is a tour stop
 * keyed to the IDE mock and its animation script.
 *
 * `target` and `side` describe where the callout card sits relative to the
 * IDE mock; the IDE mock owns its animation via the `anim` field, which is
 * resolved inside IdeMock.tsx.
 */

export type Side = 'top' | 'right' | 'bottom' | 'left';

/** Animation keys consumed by IdeMock.tsx */
export type AnimKey =
  | 'idle'
  | 'continue'        // run forward to next breakpoint
  | 'step-into'       // step into a callee
  | 'reverse'         // run backward
  | 'workspace'       // cycle activity-bar focus
  | 'breakpoints'     // expose plain bp + conditional bp ("watchpoint")
  | 'locals'          // populate locals + state vars
  | 'watch'           // switch sub-tab to Watch + populate
  | 'decompile-soon'  // teal "coming soon" badge on the bytecode-only file
  | 'snapshot-cycle'; // rapidly cycle the snapshot counter

export type StageKind = 'panel' | 'tour';

export type Stage = {
  id: string;
  kind: StageKind;
  /** label shown on the progress strip + small lower-right panel */
  label: string;
  /** longer marketing badge for the callout card */
  badge: string;
  /** card title */
  title: string;
  /** card body, supports `code spans` and **bold** */
  body: string;
  /** accent color used for ring + callout border + progress segment */
  color: string;
  /** for kind === 'tour': which IDE mock element to ring */
  target?:
    | { kind: 'selector'; selector: string }
    | { kind: 'whole-frame' };
  /** for kind === 'tour': which side the callout card sits on */
  side?: Side;
  /** for kind === 'tour': animation script to run while this stage is active */
  anim?: AnimKey;
};

export const STAGES: Stage[] = [
  // ── 0. Welcome ───────────────────────────────────────────────────────────
  {
    id: 'welcome',
    kind: 'panel',
    label: 'edb',
    badge: 'Welcome',
    title: 'edb · The Ethereum Project Debugger',
    body: 'Press → to take the tour.',
    color: '#c2410c',
  },

  // ── 1. Continue (F5) ────────────────────────────────────────────────────
  {
    id: 'continue',
    kind: 'tour',
    label: 'Continue',
    badge: 'Continue · F5',
    title: 'Run to the next breakpoint',
    body: 'Two stops are armed: a plain **breakpoint** on `whenNotPaused`, and a **watchpoint** guarded by `paused == true`. Each click of Continue runs forward until the next stop. Watch the cursor land on `234`, then on `242`.',
    color: '#d97706',
    target: { kind: 'selector', selector: '.btn-continue' },
    side: 'bottom',
    anim: 'continue',
  },

  // 2. Step Into (F11). Represents Step Over / Step Into / Step Out.
  {
    id: 'step-into',
    kind: 'tour',
    label: 'Step Into',
    badge: 'Step Into · F11',
    title: 'Step into a function call',
    body: 'Step *into* the callee. Step *over* and step *out* are the symmetric pair. Each press advances exactly one snapshot; nothing is collapsed, nothing is faked.',
    color: '#7c3aed',
    target: { kind: 'selector', selector: '.btn-step-into, .btn-step-out, .btn-step-over' },
    side: 'bottom',
    anim: 'step-into',
  },

  // ── 3. Reverse step / Reverse Continue ──────────────────────────────────
  {
    id: 'reverse',
    kind: 'tour',
    label: 'Reverse',
    badge: 'Reverse · time travel',
    title: 'Step backward through time',
    body: 'Reverse Continue rewinds to the previous breakpoint; Reverse Step undoes one snapshot. Time travel is symmetric: there is no replay drift, because the snapshot trace is fully materialised before stepping starts.',
    color: '#0d9488',
    target: { kind: 'selector', selector: '.btn-reverse-step' },
    side: 'bottom',
    anim: 'reverse',
  },

  // ── 4. Workspace / Activity bar (4 icons, no Search) ────────────────────
  {
    id: 'workspace',
    kind: 'tour',
    label: 'Workspace',
    badge: 'Workspace · 4 panes',
    title: 'Explorer · Trace · Variables · Breakpoints',
    body: 'A VS Code-style activity rail. Four panes, each a single keystroke away.',
    color: '#2b6cb0',
    target: { kind: 'selector', selector: '.ide-activity' },
    side: 'right',
    anim: 'workspace',
  },

  // 5. Locals & state.
  {
    id: 'locals',
    kind: 'tour',
    label: 'Variable Inspection',
    badge: 'Variable Inspection',
    title: 'Locals & state, fully decoded',
    body: 'Watch local variables populate as the function body executes. State variables, mappings, structs: all decoded against the live snapshot, with type annotations attached.',
    color: '#2e8b2e',
    target: { kind: 'selector', selector: '.ide-bottom' },
    side: 'top',
    anim: 'locals',
  },

  // ── 7. Watch / Expression eval ──────────────────────────────────────────
  {
    id: 'watch',
    kind: 'tour',
    label: 'Eval',
    badge: 'Expression eval',
    title: 'Watch & evaluate any expression',
    body: 'Switch to `Watch` and type any Solidity expression: function calls, arithmetic, storage reads. EDB evaluates against the current snapshot in real time.',
    color: '#7c3aed',
    target: { kind: 'selector', selector: '.ide-subtab-watch' },
    side: 'top',
    anim: 'watch',
  },

  // ── 8. Decompilation (coming soon) ──────────────────────────────────────
  {
    id: 'decompile-soon',
    kind: 'tour',
    label: 'Decompile',
    badge: 'Coming soon',
    title: 'Bytecode decompilation',
    body: "When verified source is missing, EDB will decompile raw bytecode into readable, fully-steppable pseudo-Solidity. The file `0xdac1…1ec7` here is a contract that arrived with bytecode only. *Coming soon* in a future release.",
    color: '#0d9488',
    target: { kind: 'selector', selector: '.ide-bytecode-file' },
    side: 'right',
    anim: 'decompile-soon',
  },

  // ── 9. Snapshot precision ───────────────────────────────────────────────
  {
    id: 'snapshot-precision',
    kind: 'tour',
    label: 'Precision',
    badge: 'Why precision matters',
    title: 'Snapshot N / 34',
    body: 'Every executed Solidity statement produces a snapshot: 34 of them for this transaction. Time travel is just an array index. No instrumentation, no replay drift, no surprises.',
    color: '#c2410c',
    target: { kind: 'selector', selector: '.ide-snapshot-counter' },
    side: 'top',
    anim: 'snapshot-cycle',
  },

  // 10. Outro: Why EDB + Get involved combined into one final panel.
  {
    id: 'outro',
    kind: 'panel',
    label: 'Why EDB',
    badge: 'Why EDB · Get involved',
    title: 'A debugger that finally matches Solidity',
    body: 'Other tools show you traces. EDB lets you inhabit the execution.',
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
