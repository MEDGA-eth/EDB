import { FUND_MAILTO } from './Hero';

const REPO = 'https://github.com/edb-rs/edb';
const DAPLAB = 'https://daplab.cs.columbia.edu/';
const DAPLAB_LOGO = 'https://daplab.cs.columbia.edu/files/images/daplab_logo_horiz.png';

export default function CtaStage() {
  return (
    <div className="panel">
      <div className="cta-card">
        <span className="hero-tag" style={{ fontSize: 11 }}>
          <span aria-hidden>💌</span> Get in touch <span aria-hidden>💌</span>
        </span>
        <h2 style={{ fontFamily: 'var(--font-display)', fontSize: 'clamp(28px, 4vw, 40px)', fontWeight: 700, marginTop: 8, letterSpacing: '-0.018em' }}>
          Use EDB at a company?
        </h2>
        <p style={{ marginTop: 8, marginBottom: 16, color: 'var(--fg-secondary)', fontSize: 14, lineHeight: 1.6 }}>
          EDB is built and maintained at Columbia's DAPLab as free, open-source software. If your team relies on it — or you'd like to — we'd love to hear from you about sponsorship, support contracts, or features you'd want prioritised.
        </p>
        <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap', justifyContent: 'center' }}>
          <a href={FUND_MAILTO} className="btn-primary">💼 Fund this work — zz@cs.columbia.edu</a>
          <a href={REPO} target="_blank" rel="noopener noreferrer" className="btn-secondary">★ Star on GitHub</a>
        </div>
        <div className="gh-cta" style={{ marginTop: 14, justifyContent: 'center', display: 'flex' }}>
          <span className="nudge">
            <span className="wave" aria-hidden>👈</span>
            every star helps us argue for funding!
          </span>
        </div>
        <div style={{ marginTop: 18, display: 'flex', alignItems: 'center', gap: 12, justifyContent: 'center', flexWrap: 'wrap' }}>
          <a href={DAPLAB} target="_blank" rel="noopener noreferrer">
            <img src={DAPLAB_LOGO} alt="DAPLab @ Columbia" style={{ height: 22, opacity: 0.92 }}
              onError={(e) => { (e.currentTarget as HTMLImageElement).style.display = 'none'; }} />
          </a>
          <span style={{ width: 1, height: 14, background: 'var(--border-strong)' }} />
          <span style={{ fontSize: 13, color: 'var(--fg-secondary)' }}>
            built by <a href="https://zzhang.xyz" target="_blank" rel="noopener noreferrer" style={{ color: 'var(--accent-warm)', fontWeight: 800, textDecoration: 'none' }}>Zhuo Zhang</a> &amp; Wuqi Zhang
          </span>
          <span style={{ width: 1, height: 14, background: 'var(--border-strong)' }} />
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--fg-tertiary)' }}>AGPL-3.0</span>
        </div>
      </div>
    </div>
  );
}
