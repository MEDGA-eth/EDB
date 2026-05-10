import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { STAGES, type Stage } from './data/stages';
import IdeMock from './components/IdeMock';
import HeroStage from './components/Hero';
import StrengthsStage from './components/Strengths';
import CtaStage from './components/Footer';

const AUTO_KEY = 'edb-website:auto';
const AUTO_INTERVAL_MS = 5500;

type CalloutPos = {
  ringLeft: number; ringTop: number; ringW: number; ringH: number;
  cardLeft: number; cardTop: number; cardTransform: string;
} | null;

export default function App() {
  const [idx, setIdx] = useState(0);
  const [auto, setAuto] = useState<boolean>(() => {
    if (typeof localStorage === 'undefined') return false;
    return localStorage.getItem(AUTO_KEY) === '1';
  });
  const [pos, setPos] = useState<CalloutPos>(null);

  const stage = STAGES[idx]!;
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

  /* auto-advance */
  useEffect(() => {
    if (!auto) return;
    if (typeof matchMedia !== 'undefined' && matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    const id = setInterval(() => {
      setIdx((i) => (i + 1) % STAGES.length);
    }, AUTO_INTERVAL_MS);
    return () => clearInterval(id);
  }, [auto]);

  useEffect(() => {
    try { localStorage.setItem(AUTO_KEY, auto ? '1' : '0'); } catch { /* ignore */ }
  }, [auto]);

  /* callout positioning */
  useEffect(() => {
    const root = ideMockRootRef.current;
    if (!root) { setPos(null); return; }
    if (stage.kind !== 'tour' || !stage.target) { setPos(null); return; }

    const compute = () => {
      const sel = stage.target!.kind === 'selector' ? stage.target!.selector : null;
      if (!sel) return;
      const el = root.querySelector(sel) as HTMLElement | null;
      if (!el) { setPos(null); return; }
      const r = el.getBoundingClientRect();
      const rootR = root.getBoundingClientRect();
      const ringLeft = r.left - rootR.left - 4;
      const ringTop = r.top - rootR.top - 4;
      const ringW = r.width + 8;
      const ringH = r.height + 8;
      const G = 14;
      const cardW = Math.min(320, Math.max(230, rootR.width * 0.26));
      const cardH = 160;
      let cardLeft = 0; let cardTop = 0;
      let baseTx = { x: 0, y: 0 };
      switch (stage.side) {
        case 'right':
          cardLeft = ringLeft + ringW + G; cardTop = ringTop + ringH / 2;
          baseTx = { x: 0, y: -50 }; break;
        case 'left':
          cardLeft = ringLeft - G; cardTop = ringTop + ringH / 2;
          baseTx = { x: -100, y: -50 }; break;
        case 'top':
          cardLeft = ringLeft + ringW / 2; cardTop = ringTop - G;
          baseTx = { x: -50, y: -100 }; break;
        case 'bottom':
        default:
          cardLeft = ringLeft + ringW / 2; cardTop = ringTop + ringH + G;
          baseTx = { x: -50, y: 0 }; break;
      }
      // clamp inside the mock root
      const PAD = 8;
      const boxLeft = cardLeft + (baseTx.x / 100) * cardW;
      const boxTop = cardTop + (baseTx.y / 100) * cardH;
      let nudgeX = 0, nudgeY = 0;
      if (boxLeft < PAD) nudgeX = PAD - boxLeft;
      else if (boxLeft + cardW > rootR.width - PAD)
        nudgeX = rootR.width - PAD - cardW - boxLeft;
      if (boxTop < PAD) nudgeY = PAD - boxTop;
      else if (boxTop + cardH > rootR.height - PAD)
        nudgeY = rootR.height - PAD - cardH - boxTop;
      const cardTransform = `translate(calc(${baseTx.x}% + ${nudgeX}px), calc(${baseTx.y}% + ${nudgeY}px))`;
      setPos({ ringLeft, ringTop, ringW, ringH, cardLeft, cardTop, cardTransform });
    };
    compute();
    const ro = new ResizeObserver(compute);
    ro.observe(root);
    window.addEventListener('resize', compute);
    /* second pass after layout settles */
    const t = setTimeout(compute, 80);
    return () => { ro.disconnect(); window.removeEventListener('resize', compute); clearTimeout(t); };
  }, [stage]);

  const onSegmentClick = useCallback((i: number) => setIdx(i), []);

  const tourCount = useMemo(() => STAGES.filter((s) => s.kind === 'tour').length, []);
  const tourPos = useMemo(() => {
    let n = 0;
    for (let i = 0; i <= idx; i++) if (STAGES[i]!.kind === 'tour') n++;
    return n;
  }, [idx]);

  return (
    <div className="shell">
      {/* progress strip */}
      <div className="shell-progress" role="navigation" aria-label="Tour stages">
        <div className="progress-wordmark">edb</div>
        <div className="progress-track">
          {STAGES.map((s, i) => (
            <button
              key={s.id}
              type="button"
              className={`progress-seg ${i === idx ? 'is-active' : ''}`}
              style={{ ['--seg-color' as string]: s.color } as React.CSSProperties}
              onClick={() => onSegmentClick(i)}
              aria-current={i === idx}
              title={s.label}
            >
              {s.label}
            </button>
          ))}
        </div>
        <span className="progress-counter">
          {stage.kind === 'tour' ? `${tourPos}/${tourCount}` : `· ${stage.label}`}
        </span>
        <button
          type="button"
          className={`progress-auto ${auto ? 'is-on' : ''}`}
          onClick={() => setAuto((v) => !v)}
          aria-pressed={auto}
          title="Auto-advance every ~5 seconds"
        >
          {auto ? '▶ Auto' : '⏸ Auto'}
        </button>
      </div>

      {/* stage area */}
      <div className="shell-stage" data-kind={stage.kind} data-stage={stage.id}>
        {/* IDE mock — mounted once, dimmed for panel stages */}
        <div className="ide-mock-wrap" ref={ideMockRootRef}>
          <IdeMock stage={stage.id} anim={stage.kind === 'tour' ? (stage.anim ?? 'idle') : 'idle'} dim={stage.kind !== 'tour'} />

          {/* ring + callout overlay (only when target available) */}
          {stage.kind === 'tour' && pos && (
            <>
              <div
                className="tour-ring"
                style={{
                  left: pos.ringLeft, top: pos.ringTop,
                  width: pos.ringW, height: pos.ringH,
                  ['--ring-color' as string]: stage.color,
                } as React.CSSProperties}
                aria-hidden
              />
              <div
                key={stage.id}
                className="tour-callout"
                style={{
                  left: pos.cardLeft, top: pos.cardTop,
                  transform: pos.cardTransform,
                }}
              >
                <div className="tour-callout-card" style={{ ['--callout-color' as string]: stage.color } as React.CSSProperties}>
                  <div className="tour-callout-badge">
                    <span className="num">{tourPos}</span>
                    <span>{stage.badge}</span>
                  </div>
                  <div className="tour-callout-title">{stage.title}</div>
                  <div className="tour-callout-text" dangerouslySetInnerHTML={{ __html: renderInline(stage.body) }} />
                </div>
              </div>
            </>
          )}
        </div>

        {/* panel content (hero, strengths, cta) */}
        <div className="panel-wrap">
          {stage.id === 'welcome' && <HeroStage />}
          {stage.id === 'strengths' && <StrengthsStage />}
          {stage.id === 'cta' && <CtaStage />}
        </div>
      </div>

      {/* footer status */}
      <div className="shell-status">
        <span>← / → to navigate</span>
        <span>· Space ▶</span>
        <span>· Home/End to jump</span>
        <span style={{ marginLeft: 'auto' }}>
          built at <a href="https://daplab.cs.columbia.edu/" target="_blank" rel="noopener noreferrer" style={{ color: 'inherit', fontWeight: 700 }}>DAPLab @ Columbia</a> · <a href="https://github.com/edb-rs/edb" target="_blank" rel="noopener noreferrer" style={{ color: 'inherit', fontWeight: 700 }}>edb-rs/edb</a>
        </span>
      </div>
    </div>
  );
}

function renderInline(s: string): string {
  return s
    .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
    .replace(/`([^`]+)`/g, '<code>$1</code>')
    .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
}
