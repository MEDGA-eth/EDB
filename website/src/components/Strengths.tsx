const STRENGTHS: { emoji: string; title: string; body: string; accent: string; hint?: string }[] = [
  { emoji: '🎯', title: 'Snapshot-level stepping', body: 'Every individual EVM operation is a snapshot you can step into, step over, or rewind. Time travel is just an array index.', accent: '#d97706', hint: 'F5 · F10 · F11' },
  { emoji: '🔍', title: 'Locals & state, decoded', body: 'Locals populate live; state vars, mappings, structs are all decoded against the current snapshot, with type tags.', accent: '#2b6cb0', hint: 'Variables panel' },
  { emoji: '🧮', title: 'Expression evaluation', body: 'Type any Solidity expression — calls, arithmetic, storage reads — evaluated against the live snapshot.', accent: '#2e8b2e', hint: 'Watch · REPL' },
  { emoji: '🔴', title: 'Breakpoints & watchpoints', body: 'Pause on any line. Attach a Solidity expression to upgrade a breakpoint into a watchpoint that fires on a value.', accent: '#7c3aed', hint: 'b · cond' },
  { emoji: '🔓', title: 'Bytecode decompilation (soon)', body: 'No verified source? EDB will turn raw bytecode into readable, fully-steppable pseudo-Solidity. Shipping in a future release.', accent: '#0d9488', hint: 'coming soon' },
  { emoji: '⏪', title: 'Forward & reverse, equally', body: 'Reverse Continue, Reverse Step, Prev/Next Call. Time travel is symmetric — no replay drift.', accent: '#d4608a', hint: 'Reverse · Continue' },
];

export default function StrengthsStage() {
  return (
    <div className="panel">
      <div style={{ textAlign: 'center', marginBottom: 16 }}>
        <span className="hero-tag" style={{ fontSize: 11 }}>
          <span aria-hidden>✨</span> Why EDB <span aria-hidden>✨</span>
        </span>
        <h2 style={{ fontFamily: 'var(--font-display)', fontSize: 'clamp(28px, 4vw, 38px)', fontWeight: 700, marginTop: 8, letterSpacing: '-0.015em' }}>
          A debugger that finally matches Solidity
        </h2>
      </div>
      <div className="strength-grid">
        {STRENGTHS.map((s) => (
          <div key={s.title} className="strength-card" style={{ ['--card-accent' as string]: s.accent } as React.CSSProperties}>
            <div className="strength-icon">{s.emoji}</div>
            <div className="strength-title">{s.title}</div>
            <div className="strength-text">{s.body}</div>
            {s.hint && <div className="strength-hint">{s.hint}</div>}
          </div>
        ))}
      </div>
    </div>
  );
}
