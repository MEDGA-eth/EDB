interface Props {
  onPrev: () => void;
  onNext: () => void;
  prevDisabled: boolean;
  nextDisabled: boolean;
}

export default function MobileNav({ onPrev, onNext, prevDisabled, nextDisabled }: Props) {
  return (
    <>
      <button
        type="button"
        className="mobile-nav-btn mobile-nav-prev"
        onClick={onPrev}
        disabled={prevDisabled}
        aria-label="Previous stage"
      >
        <Chevron dir="left" />
      </button>
      <button
        type="button"
        className="mobile-nav-btn mobile-nav-next"
        onClick={onNext}
        disabled={nextDisabled}
        aria-label="Next stage"
      >
        <Chevron dir="right" />
      </button>
    </>
  );
}

function Chevron({ dir }: { dir: 'left' | 'right' }) {
  return (
    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" aria-hidden>
      <path
        d={dir === 'left' ? 'M 15 6 L 9 12 L 15 18' : 'M 9 6 L 15 12 L 9 18'}
        stroke="currentColor"
        strokeWidth="2.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}
