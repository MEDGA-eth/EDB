import { useEffect, useState } from 'react';

/** Match phones held in portrait. The triple-AND ensures we never trigger
 *  on desktops (pointer: fine) or on tablets in portrait wider than 720px.
 *  The breakpoint matches the mobile-overrides @media block in index.css
 *  so JS and CSS agree on what "mobile" means. */
const ROTATE_QUERY = '(orientation: portrait) and (max-width: 720px) and (pointer: coarse)';

export default function RotateOverlay() {
  const [matches, setMatches] = useState<boolean>(() => {
    if (typeof window === 'undefined' || typeof window.matchMedia === 'undefined') {
      return false;
    }
    return window.matchMedia(ROTATE_QUERY).matches;
  });
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    if (typeof window === 'undefined' || typeof window.matchMedia === 'undefined') return;
    const mql = window.matchMedia(ROTATE_QUERY);
    setMatches(mql.matches);
    const onChange = (e: MediaQueryListEvent) => {
      setMatches(e.matches);
      // Reset the dismiss flag whenever the device leaves the portrait-phone
      // bucket, so rotating back to portrait re-pops the prompt. Per spec:
      // dismissal is per-portrait-session, not sticky.
      if (!e.matches) setDismissed(false);
    };
    mql.addEventListener('change', onChange);
    return () => mql.removeEventListener('change', onChange);
  }, []);

  if (!matches || dismissed) return null;

  return (
    <div className="rotate-overlay" role="dialog" aria-label="Rotate device for the EDB tour">
      <div className="rotate-card">
        <div className="rotate-illustration" aria-hidden>
          <PhoneArrow />
        </div>
        <p className="rotate-caption">
          <strong>EDB tour fits best in landscape.</strong>
          <span>Rotate your phone to continue.</span>
        </p>
        <button
          type="button"
          className="rotate-skip"
          onClick={() => setDismissed(true)}
        >
          Continue in portrait anyway
        </button>
      </div>
    </div>
  );
}

/** Phone outline that rotates via CSS keyframes, paired with a curved arrow
 *  that points clockwise to indicate the gesture direction. */
function PhoneArrow() {
  return (
    <svg width="120" height="120" viewBox="0 0 120 120" fill="none" aria-hidden>
      <g className="rotate-phone">
        <rect
          x="44" y="20" width="32" height="80" rx="6"
          stroke="currentColor" strokeWidth="2.5"
          fill="rgba(255, 255, 255, 0.65)"
        />
        <line x1="55" y1="28" x2="65" y2="28"
          stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
        <circle cx="60" cy="92" r="2" fill="currentColor" />
      </g>
      <path
        d="M 96 60 A 32 32 0 0 1 76 96"
        stroke="currentColor" strokeWidth="2.5" fill="none" strokeLinecap="round"
      />
      <path
        d="M 76 96 L 82 88 M 76 96 L 68 96"
        stroke="currentColor" strokeWidth="2.5" fill="none"
        strokeLinecap="round" strokeLinejoin="round"
      />
    </svg>
  );
}
