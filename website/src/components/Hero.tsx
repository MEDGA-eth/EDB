const FUND_SUBJECT = encodeURIComponent('EDB sponsorship — funding inquiry');
const FUND_BODY = encodeURIComponent(
  'Hi Zhuo,\n\nI represent [COMPANY], and we use EDB / are interested in sponsoring its continued development. Could we set up a short call to discuss?\n\nThanks,\n[YOUR NAME]\n',
);
export const FUND_MAILTO = `mailto:zz@cs.columbia.edu?subject=${FUND_SUBJECT}&body=${FUND_BODY}`;

const REPO = 'https://github.com/edb-rs/edb';

export default function HeroStage() {
  return (
    <div className="panel grid">
      <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 14, maxWidth: 980 }}>
        <span className="hero-tag">
          <span aria-hidden>✨</span>
          Ethereum · Solidity · Time-travel debugger
          <span aria-hidden>✨</span>
        </span>
        <h1 className="hero-title">edb</h1>
        <p className="hero-lede">
          A time-travel debugger for Solidity. Step through your <strong style={{ color: 'var(--accent-warm)' }}>local Foundry tests</strong>, or <strong style={{ color: 'var(--accent-warm)' }}>replay any on-chain transaction</strong>, at the EVM-snapshot level — locals, watches, expression evaluation, breakpoints, and a built-in decompiler for bytecode-only contracts (coming soon).
        </p>
        <div style={{ display: 'flex', gap: 10, flexWrap: 'wrap', justifyContent: 'center', marginTop: 8 }}>
          {[
            ['Snapshot stepping', '#d97706'],
            ['Locals & state', '#2b6cb0'],
            ['Expression eval', '#2e8b2e'],
            ['Watchpoints', '#7c3aed'],
            ['Decompile · soon', '#0d9488'],
          ].map(([label, color]) => (
            <span key={label as string} style={{
              display: 'inline-flex', alignItems: 'center', gap: 6,
              padding: '4px 10px', borderRadius: 999,
              fontFamily: 'var(--font-mono)', fontSize: 12, fontWeight: 700, letterSpacing: '0.04em',
              color: color as string,
              border: `1.5px solid ${color}`,
              background: `color-mix(in srgb, ${color} 8%, transparent)`,
            }}>
              <span style={{ width: 6, height: 6, borderRadius: '50%', background: color as string }} />
              {label as string}
            </span>
          ))}
        </div>
        <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap', justifyContent: 'center', marginTop: 16 }}>
          <a href={REPO} target="_blank" rel="noopener noreferrer" className="btn-secondary">
            <span aria-hidden>★</span> github.com/edb-rs/edb
          </a>
          <a href={FUND_MAILTO} className="btn-primary">
            <span aria-hidden>💼</span> Are you a company? Fund this work
          </a>
        </div>
        <div className="gh-cta" style={{ marginTop: 14 }}>
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 12.5, color: 'var(--fg-tertiary)', fontWeight: 600 }}>
            press
          </span>
          <span className="kbd">←</span>
          <span className="kbd">→</span>
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 12.5, color: 'var(--fg-tertiary)', fontWeight: 600 }}>
            to take the tour
          </span>
          <span className="nudge">
            <span className="wave" aria-hidden>👉</span>
            try it!
          </span>
        </div>
      </div>
    </div>
  );
}
