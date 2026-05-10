import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { STAGES } from './data/stages';
import IdeMock from './components/IdeMock';
import HeroStage, { FUND_MAILTO } from './components/Hero';
import StrengthsStage from './components/Strengths';
import RotateOverlay from './components/RotateOverlay';
import MobileSheet from './components/MobileSheet';
import MobileNav from './components/MobileNav';
import { useIsMobile } from './lib/useIsMobile';

type RingPos = {
  left: number;
  top: number;
  width: number;
  height: number;
} | null;

export default function App() {
  const [idx, setIdx] = useState(0);
  const [ring, setRing] = useState<RingPos>(null);
  const isMobile = useIsMobile();
  const [sheetOpen, setSheetOpen] = useState(true);

  const stage = STAGES[idx]!;

  /* Auto-open the mobile sheet whenever the stage changes. On welcome /
     outro panels MobileSheet renders null, so this is harmless there. */
  useEffect(() => {
    setSheetOpen(true);
  }, [idx]);
  const ideMockRootRef = useRef<HTMLDivElement>(null);

  /* keyboard */
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement | null)?.tagName;
      if (tag === 'INPUT' || tag === 'TEXTAREA') return;
      if (e.key === 'ArrowRight' || e.key === ' ') {
        e.preventDefault();
        setIdx((i) => Math.min(i + 1, STAGES.length - 1));
      } else if (e.key === 'ArrowLeft') {
        e.preventDefault();
        setIdx((i) => Math.max(i - 1, 0));
      } else if (e.key === 'Home') {
        e.preventDefault();
        setIdx(0);
      } else if (e.key === 'End') {
        e.preventDefault();
        setIdx(STAGES.length - 1);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  /* (auto-advance was here; the header now hosts a permanent funding-request
     pill instead, so the keep-the-ask-visible job is handled by surface
     real estate rather than a toggle that times out.) */

  /* ring positioning. The colored highlight on the IDE; the description
     text now lives in the right rail. */
  useEffect(() => {
    const root = ideMockRootRef.current;
    if (!root) { setRing(null); return; }
    if (stage.kind !== 'tour' || !stage.target) { setRing(null); return; }

    const compute = () => {
      const sel = stage.target!.kind === 'selector' ? stage.target!.selector : null;
      if (!sel) return;
      // Comma-separated selectors are unioned into one ring rect, so a stage
      // can highlight a related group (e.g., all three step buttons).
      const els = root.querySelectorAll(sel);
      if (els.length === 0) { setRing(null); return; }
      let l = Infinity, t = Infinity, r = -Infinity, b = -Infinity;
      els.forEach((el) => {
        const cr = (el as HTMLElement).getBoundingClientRect();
        l = Math.min(l, cr.left);
        t = Math.min(t, cr.top);
        r = Math.max(r, cr.right);
        b = Math.max(b, cr.bottom);
      });
      const rootR = root.getBoundingClientRect();
      setRing({
        left: l - rootR.left - 4,
        top: t - rootR.top - 4,
        width: (r - l) + 8,
        height: (b - t) + 8,
      });
    };
    compute();
    const ro = new ResizeObserver(compute);
    ro.observe(root);
    window.addEventListener('resize', compute);
    /* second pass after layout settles (animations move things) */
    const t = setTimeout(compute, 80);
    return () => { ro.disconnect(); window.removeEventListener('resize', compute); clearTimeout(t); };
  }, [stage]);

  const tourCount = useMemo(() => STAGES.filter((s) => s.kind === 'tour').length, []);
  const tourPos = useMemo(() => {
    let n = 0;
    for (let i = 0; i <= idx; i++) if (STAGES[i]!.kind === 'tour') n++;
    return n;
  }, [idx]);

  const onPrev = useCallback(() => setIdx((i) => Math.max(i - 1, 0)), []);
  const onNext = useCallback(
    () => setIdx((i) => Math.min(i + 1, STAGES.length - 1)),
    [],
  );
  const isFirst = idx === 0;
  const isLast = idx === STAGES.length - 1;

  return (
    <>
      <div className="shell" data-mobile={isMobile ? 'true' : 'false'}>
      {/* header. Wordmark on the left (click = back to welcome), transport
          toggle on the right. */}
      <div className="shell-header">
        <button
          type="button"
          className="shell-wordmark"
          onClick={() => setIdx(0)}
          aria-label="Back to the welcome page"
          title="Back to the welcome page"
        >
          <span className="shell-wordmark-mark">edb</span>
          <span className="shell-wordmark-sub">The Ethereum Project Debugger</span>
        </button>
        <div className="shell-header-tools">
          <a
            className="shell-link"
            href="https://github.com/edb-rs/edb"
            target="_blank"
            rel="noopener noreferrer"
            title="github.com/edb-rs/edb"
          >
            <GhIcon /> edb-rs/edb
          </a>
          <a
            className="shell-fund"
            href={FUND_MAILTO}
            aria-label="Send a funding inquiry to zz@cs.columbia.edu"
            title="EDB needs funding · click to send a sponsorship email"
          >
            <span aria-hidden>🙏</span>
            Sponsor EDB
          </a>
        </div>
      </div>

      {/* body. IDE on the left, info rail on the right (rail hidden on panel stages). */}
      <div
        className="shell-body"
        data-kind={stage.kind}
        data-stage={stage.id}
      >
        <div className="shell-stage">
          {/* the IDE mock is mounted once and dimmed for panel stages */}
          <div className="ide-mock-wrap" ref={ideMockRootRef}>
            <IdeMock
              stage={stage.id}
              anim={stage.kind === 'tour' ? (stage.anim ?? 'idle') : 'idle'}
              dim={stage.kind !== 'tour'}
            />
            {stage.kind === 'tour' && ring && (
              <div
                className="tour-ring"
                style={
                  {
                    left: ring.left,
                    top: ring.top,
                    width: ring.width,
                    height: ring.height,
                    ['--ring-color' as string]: stage.color,
                  } as React.CSSProperties
                }
                aria-hidden
              />
            )}
          </div>

          {/* panel stages (welcome / outro) overlay the dimmed mock */}
          <div className="panel-wrap">
            {stage.id === 'welcome' && <HeroStage />}
            {stage.id === 'outro' && <StrengthsStage onRestart={() => setIdx(0)} />}
          </div>
        </div>

        {/* info rail (only rendered for tour stages; panel stages get the
            full body width so the hero / strengths / cta breathe). */}
        {stage.kind === 'tour' && (
        <aside
          className="shell-rail"
          aria-label="Stage description"
          style={{ ['--rail-color' as string]: stage.color } as React.CSSProperties}
        >
          {/* head: stage number + back arrow + badge + title */}
          <div className="rail-head">
            <div className="rail-num">
              <span className="rail-num-cur">{String(tourPos).padStart(2, '0')}</span>
              <span className="rail-num-sep">/</span>
              <span className="rail-num-tot">{String(tourCount).padStart(2, '0')}</span>
            </div>
            <div className="rail-badge-row">
              <span className="rail-back-arrow" aria-hidden>
                <BackArrow />
              </span>
              <span className="rail-badge">{stage.badge}</span>
            </div>
            <h2 className="rail-title">{stage.title}</h2>
          </div>

          {/* body: rich JSX from each stage's body field. Scrolls inside the
              rail so long explanations don't push the nav off-screen. */}
          <div className="rail-body" key={stage.id}>
            {stage.body}
          </div>

          {/* foot: stage pips + nav buttons. Pinned to the bottom of the rail. */}
          <div className="rail-foot">
            <div className="rail-stage-pips" aria-hidden>
              {STAGES.map((s, i) => {
                if (s.kind !== 'tour') return null;
                return (
                  <button
                    key={s.id}
                    type="button"
                    className={`rail-pip ${i === idx ? 'is-active' : ''}`}
                    style={{ ['--pip-color' as string]: s.color } as React.CSSProperties}
                    onClick={() => setIdx(i)}
                    title={s.label}
                    aria-label={`Jump to ${s.label}`}
                  />
                );
              })}
            </div>
            <div className="rail-nav">
              <button
                type="button"
                className="rail-nav-btn"
                onClick={onPrev}
                disabled={isFirst}
                aria-label="Previous stage"
              >
                ← prev
              </button>
              <span className="rail-nav-keys">
                <span className="kbd">←</span>
                <span className="kbd">→</span>
              </span>
              <button
                type="button"
                className="rail-nav-btn"
                onClick={onNext}
                disabled={isLast}
                aria-label="Next stage"
              >
                next →
              </button>
            </div>
          </div>
        </aside>
        )}
      </div>
    </div>
    {isMobile && (
      <>
        <MobileSheet
          stage={stage}
          tourPos={tourPos}
          tourCount={tourCount}
          open={sheetOpen}
          onDismiss={() => setSheetOpen(false)}
        />
        <MobileNav
          onPrev={onPrev}
          onNext={onNext}
          prevDisabled={isFirst}
          nextDisabled={isLast}
        />
      </>
    )}
    <RotateOverlay />
    </>
  );
}

function GhIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden>
      <path d="M12 0c-6.626 0-12 5.373-12 12 0 5.302 3.438 9.8 8.207 11.387.599.111.793-.261.793-.577v-2.234c-3.338.726-4.033-1.416-4.033-1.416-.546-1.387-1.333-1.756-1.333-1.756-1.089-.745.083-.729.083-.729 1.205.084 1.839 1.237 1.839 1.237 1.07 1.834 2.807 1.304 3.492.997.107-.775.418-1.305.762-1.604-2.665-.305-5.467-1.334-5.467-5.931 0-1.311.469-2.381 1.236-3.221-.124-.303-.535-1.524.117-3.176 0 0 1.008-.322 3.301 1.23.957-.266 1.983-.399 3.003-.404 1.02.005 2.047.138 3.006.404 2.291-1.552 3.297-1.23 3.297-1.23.653 1.653.242 2.874.118 3.176.77.84 1.235 1.911 1.235 3.221 0 4.609-2.807 5.624-5.479 5.921.43.372.823 1.102.823 2.222v3.293c0 .319.192.694.801.576 4.765-1.589 8.199-6.086 8.199-11.386 0-6.627-5.373-12-12-12z" />
    </svg>
  );
}

/** Curved back-arrow that points from the rail toward the IDE on the left. */
function BackArrow() {
  return (
    <svg width="22" height="22" viewBox="0 0 32 32" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M27 24 C 27 16, 21 9, 9 7" />
      <path d="M11 3 L 6 7 L 11 11" />
    </svg>
  );
}
