import { useEffect, useState } from 'react';

/**
 * Mobile breakpoint, shared between JS and CSS.
 *
 * The CSS-side equivalent is the `@media (max-width: 720px)` block at the
 * end of index.css. The JS hook adds the orientation/height/pointer
 * guards because it controls things that interrupt the user (rotate
 * overlay, popup sheets) and we want the stricter "actually a phone"
 * check there.
 */
export const MOBILE_QUERY =
  '(pointer: coarse) and ((max-width: 720px) or (max-height: 500px))';

export function useIsMobile(): boolean {
  const [matches, setMatches] = useState<boolean>(() => {
    if (typeof window === 'undefined' || typeof window.matchMedia === 'undefined') {
      return false;
    }
    return window.matchMedia(MOBILE_QUERY).matches;
  });

  useEffect(() => {
    if (typeof window === 'undefined' || typeof window.matchMedia === 'undefined') return;
    const mql = window.matchMedia(MOBILE_QUERY);
    setMatches(mql.matches);
    const onChange = (e: MediaQueryListEvent) => setMatches(e.matches);
    mql.addEventListener('change', onChange);
    return () => mql.removeEventListener('change', onChange);
  }, []);

  return matches;
}
