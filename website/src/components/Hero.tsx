const FUND_SUBJECT = encodeURIComponent('EDB · saying hi');
const FUND_BODY = encodeURIComponent(
  'Hi Zhuo,\n\nI represent [COMPANY], and we use EDB / are interested in the project. Could we set up a quick chat?\n\nThanks,\n[YOUR NAME]\n',
);
export const FUND_MAILTO = `mailto:zz@cs.columbia.edu?subject=${FUND_SUBJECT}&body=${FUND_BODY}`;

const REPO = 'https://github.com/edb-rs/edb';
const DAPLAB = 'https://daplab.cs.columbia.edu/';
const DAPLAB_LOGO =
  'https://daplab.cs.columbia.edu/files/images/daplab_logo_horiz.png';
const AUTHOR_ZZ = 'https://zzhang.xyz';
const AUTHOR_WQ = 'https://troublor.xyz/';

/** Inline coloured highlight inside card body text. The colour comes from
 *  the parent card's `--card-accent`, so the emphasis stays on-theme. */
function Hi({ children }: { children: React.ReactNode }) {
  return <strong className="hero-card-hi">{children}</strong>;
}

const HIGHLIGHTS: { emoji: string; title: string; desc: React.ReactNode; accent: string }[] = [
  {
    emoji: '🎯',
    title: 'Snapshot stepping',
    desc: (
      <>
        Step through Solidity, <Hi>perfectly</Hi> accurate at every executed
        statement. Forward and reverse, no replay drift.
      </>
    ),
    accent: '#d97706',
  },
  {
    emoji: '🔴',
    title: 'Breakpoints & watchpoints',
    desc: (
      <>
        Pause on any line. Attach a Solidity expression to upgrade a
        breakpoint into a <Hi>watchpoint</Hi> that fires on a value.
      </>
    ),
    accent: '#7c3aed',
  },
  {
    emoji: '🧮',
    title: 'Solidity REPL',
    desc: (
      <>
        Evaluate any Solidity expression. <Hi>Perfectly</Hi> accurate, on
        every snapshot.
      </>
    ),
    accent: '#2e8b2e',
  },
];

const BUILT_ON: { name: string; url: string; color: string }[] = [
  { name: 'foundry', url: 'https://github.com/foundry-rs/foundry', color: '#c03020' },
  { name: 'revm',    url: 'https://github.com/bluealloy/revm',     color: '#7c3aed' },
  { name: 'alloy',   url: 'https://github.com/alloy-rs/alloy',     color: '#2b6cb0' },
];

function GhIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden>
      <path d="M12 0c-6.626 0-12 5.373-12 12 0 5.302 3.438 9.8 8.207 11.387.599.111.793-.261.793-.577v-2.234c-3.338.726-4.033-1.416-4.033-1.416-.546-1.387-1.333-1.756-1.333-1.756-1.089-.745.083-.729.083-.729 1.205.084 1.839 1.237 1.839 1.237 1.07 1.834 2.807 1.304 3.492.997.107-.775.418-1.305.762-1.604-2.665-.305-5.467-1.334-5.467-5.931 0-1.311.469-2.381 1.236-3.221-.124-.303-.535-1.524.117-3.176 0 0 1.008-.322 3.301 1.23.957-.266 1.983-.399 3.003-.404 1.02.005 2.047.138 3.006.404 2.291-1.552 3.297-1.23 3.297-1.23.653 1.653.242 2.874.118 3.176.77.84 1.235 1.911 1.235 3.221 0 4.609-2.807 5.624-5.479 5.921.43.372.823 1.102.823 2.222v3.293c0 .319.192.694.801.576 4.765-1.589 8.199-6.086 8.199-11.386 0-6.627-5.373-12-12-12z" />
    </svg>
  );
}

function Sparkle() {
  return (
    <span className="hero-sparkle" aria-hidden>
      ✨
    </span>
  );
}

export default function HeroStage() {
  return (
    <div className="panel hero-panel">
      <div className="hero-stack reveal-stagger">
        {/* tagline */}
        <div className="hero-tagline">
          <Sparkle />
          <span>Ethereum · Solidity · Time-travel debugger</span>
          <Sparkle />
        </div>

        {/* title with cute mascot icon */}
        <div className="hero-title-row">
          <img
            src="/edb-icon.svg"
            alt=""
            className="hero-mark"
            width="72"
            height="72"
          />
          <h1 className="hero-bigtitle">edb</h1>
        </div>

        {/* one-sentence subtitle that names the headline mission */}
        <p className="hero-lede">
          Bringing the{' '}
          <strong className="hero-lede-emph">best web2 debugging experience</strong>
          {' '}to web3.
        </p>


        {/* 3 highlight cards */}
        <div className="hero-cards">
          {HIGHLIGHTS.map((h) => (
            <div
              key={h.title}
              className="hero-card"
              style={{ ['--card-accent' as string]: h.accent } as React.CSSProperties}
            >
              <div className="hero-card-icon">{h.emoji}</div>
              <div className="hero-card-title">{h.title}</div>
              <div className="hero-card-desc">{h.desc}</div>
            </div>
          ))}
        </div>

        {/* author + DAPLab + GitHub with cute pointer */}
        <div className="hero-credits-row">
          <span className="hero-credits-by">
            by{' '}
            <a
              href={AUTHOR_ZZ}
              target="_blank"
              rel="noopener noreferrer"
              className="hero-credits-link"
            >
              Zhuo Zhang
            </a>
            {' & '}
            <a
              href={AUTHOR_WQ}
              target="_blank"
              rel="noopener noreferrer"
              className="hero-credits-link"
            >
              Wuqi Zhang
            </a>
          </span>
          <span className="hero-credits-sep" />
          <a href={DAPLAB} target="_blank" rel="noopener noreferrer" title="DAPLab @ Columbia">
            <img
              src={DAPLAB_LOGO}
              alt="DAPLab @ Columbia"
              className="hero-daplab"
              onError={(e) => {
                (e.currentTarget as HTMLImageElement).style.display = 'none';
              }}
            />
          </a>
          <span className="hero-credits-sep" />
          <span className="gh-cta">
            <a
              href={REPO}
              target="_blank"
              rel="noopener noreferrer"
              className="btn-secondary hero-gh"
            >
              <GhIcon />
              GitHub
            </a>
            <span className="nudge">
              <span className="wave" aria-hidden>👈</span>
              check the repo!
            </span>
          </span>
        </div>

        {/* built-on row, mirrors tiny-dec's "built on ideas from" */}
        <div className="hero-builton">
          <span className="hero-builton-label">
            <span aria-hidden>💡</span> Built on
          </span>
          {BUILT_ON.map((tool) => (
            <a
              key={tool.name}
              href={tool.url}
              target="_blank"
              rel="noopener noreferrer"
              className="hero-builton-pill"
              style={{
                color: tool.color,
                borderColor: `color-mix(in srgb, ${tool.color} 35%, transparent)`,
                background: `color-mix(in srgb, ${tool.color} 6%, transparent)`,
              }}
            >
              {tool.name}
            </a>
          ))}
          <span className="hero-credits-sep" />
          <a href={FUND_MAILTO} className="hero-funding">
            <span aria-hidden>🙏</span>
            EDB needs help · companies & sponsors welcome
          </a>
        </div>

        {/* press → hint, mirrors tiny-dec's "Press → to begin your journey!" */}
        <div className="hero-begin">
          <span aria-hidden>👉</span>
          <span>Press</span>
          <span className="kbd">→</span>
          <span>to take the tour</span>
        </div>
      </div>
    </div>
  );
}
