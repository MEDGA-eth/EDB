import type { Stage } from '../data/stages';

interface Props {
  stage: Stage;
  tourPos: number;
  tourCount: number;
  open: boolean;
  onDismiss: () => void;
}

export default function MobileSheet({ stage, tourPos, tourCount, open, onDismiss }: Props) {
  if (!open) return null;
  if (stage.kind !== 'tour') return null;

  return (
    <div
      className="mobile-sheet"
      role="dialog"
      aria-label={`Stage ${tourPos} of ${tourCount}: ${stage.title}`}
      onClick={onDismiss}
      style={{ ['--rail-color' as string]: stage.color } as React.CSSProperties}
    >
      <div className="mobile-sheet-hint" aria-hidden>
        <span>tap anywhere to dismiss</span>
      </div>
      <div className="mobile-sheet-card">
        <div className="mobile-sheet-num">
          <span className="mobile-sheet-num-cur">{String(tourPos).padStart(2, '0')}</span>
          <span className="mobile-sheet-num-sep">/</span>
          <span className="mobile-sheet-num-tot">{String(tourCount).padStart(2, '0')}</span>
        </div>
        <div className="mobile-sheet-badge">{stage.badge}</div>
        <h2 className="mobile-sheet-title">{stage.title}</h2>
        <div className="mobile-sheet-body">{stage.body}</div>
      </div>
    </div>
  );
}
