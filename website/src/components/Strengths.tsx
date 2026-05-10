import { FUND_MAILTO } from './Hero';

const REPO = 'https://github.com/edb-rs/edb';
const DAPLAB = 'https://daplab.cs.columbia.edu/';
const DAPLAB_LOGO =
  'https://daplab.cs.columbia.edu/files/images/daplab_logo_horiz.png';
const AUTHOR_ZZ = 'https://zzhang.xyz';
const AUTHOR_WQ = 'https://troublor.xyz/';

const STRENGTHS: { emoji: string; title: string; body: string; accent: string; hint?: string }[] = [
  {
    emoji: '🎯',
    title: 'Snapshot-level stepping',
    body: 'Every executed Solidity statement is a snapshot you can step into, step over, or rewind. Time travel is just an array index.',
    accent: '#d97706',
    hint: 'Time travel',
  },
  {
    emoji: '🔍',
    title: 'Variable Inspection',
    body: 'Locals populate live; state vars, mappings, structs are all decoded against the current snapshot, with type tags.',
    accent: '#2b6cb0',
    hint: 'Live decoded',
  },
  {
    emoji: '🧮',
    title: 'Expression evaluation',
    body: 'Type any Solidity expression: calls, arithmetic, storage reads. Evaluated against the live snapshot.',
    accent: '#2e8b2e',
    hint: 'Solidity REPL',
  },
  {
    emoji: '🔴',
    title: 'Breakpoints & watchpoints',
    body: 'Pause on any line. Attach a Solidity expression to upgrade a breakpoint into a watchpoint that fires on a value.',
    accent: '#7c3aed',
    hint: 'Conditional',
  },
  {
    emoji: '🔓',
    title: 'Bytecode decompilation',
    body: 'No verified source? EDB will turn raw bytecode into readable, fully-steppable pseudo-Solidity. Shipping in a future release.',
    accent: '#0d9488',
    hint: 'Coming soon',
  },
  {
    emoji: '⏪',
    title: 'Forward & reverse, equally',
    body: 'Reverse Continue, Reverse Step, Prev/Next Call. Time travel is symmetric: no replay drift.',
    accent: '#d4608a',
    hint: 'Bidirectional',
  },
];

export default function StrengthsStage({ onRestart }: { onRestart?: () => void }) {
  return (
    <div className="panel outro-panel">
      {/* title — single tight line so the cards have room to breathe */}
      <div className="outro-head">
        <h2 className="outro-title">
          A debugger that finally <span className="gradient-text">matches Solidity</span>.
        </h2>
      </div>

      {/* strength grid */}
      <div className="strength-grid outro-grid">
        {STRENGTHS.map((s) => (
          <div
            key={s.title}
            className="strength-card outro-card"
            style={{ ['--card-accent' as string]: s.accent } as React.CSSProperties}
          >
            <div className="strength-icon">{s.emoji}</div>
            <div className="strength-title">{s.title}</div>
            <div className="strength-text">{s.body}</div>
            {s.hint && <div className="strength-hint">{s.hint}</div>}
          </div>
        ))}
      </div>

      {/* divider */}
      <div className="outro-divider" aria-hidden />

      {/* CTA + credits */}
      <div className="outro-cta">
        <div className="outro-cta-text">
          <h3 className="outro-cta-title">
            EDB could use your <span className="gradient-text">support</span>
          </h3>
          <p className="outro-cta-body">
            EDB is built and maintained at Columbia&apos;s DAPLab as free,
            open-source software, and we&apos;re honestly{' '}
            <strong style={{ color: 'var(--accent-warm)', fontWeight: 800 }}>
              short on funding
            </strong>{' '}
            to keep this project growing. If you&apos;re using EDB at a
            company, or you could sponsor a feature, an integration, or just
            want to chat, we&apos;d love to hear from you.
          </p>
        </div>
        <div className="outro-cta-actions">
          <a href={FUND_MAILTO} className="btn-primary">
            <span aria-hidden>🙏</span>
            Sponsor &amp; say hi
          </a>
          <a href={REPO} target="_blank" rel="noopener noreferrer" className="btn-secondary">
            <span aria-hidden>★</span> Star on GitHub
          </a>
          {onRestart && (
            <button
              type="button"
              className="btn-restart"
              onClick={onRestart}
              aria-label="Restart the tour from the welcome page"
              title="Take the tour again (or press Home)"
            >
              <span aria-hidden style={{ fontSize: 16 }}>↺</span>
              Take the tour again
            </button>
          )}
        </div>
      </div>

      {/* credits row */}
      <div className="outro-credits">
        <a href={DAPLAB} target="_blank" rel="noopener noreferrer" title="DAPLab @ Columbia University">
          <img
            src={DAPLAB_LOGO}
            alt="DAPLab @ Columbia"
            style={{ height: 22, opacity: 0.92 }}
            onError={(e) => {
              (e.currentTarget as HTMLImageElement).style.display = 'none';
            }}
          />
        </a>
        <span className="outro-credits-sep" />
        <span className="outro-credits-text">
          built by{' '}
          <a
            href={AUTHOR_ZZ}
            target="_blank"
            rel="noopener noreferrer"
            style={{ color: 'var(--accent-warm)', fontWeight: 800, textDecoration: 'none' }}
          >
            Zhuo Zhang
          </a>{' '}
          &amp;{' '}
          <a
            href={AUTHOR_WQ}
            target="_blank"
            rel="noopener noreferrer"
            style={{ color: 'var(--accent-warm)', fontWeight: 800, textDecoration: 'none' }}
          >
            Wuqi Zhang
          </a>
        </span>
        <span className="outro-credits-sep" />
        <span className="outro-credits-license">AGPL-3.0</span>
      </div>
    </div>
  );
}
