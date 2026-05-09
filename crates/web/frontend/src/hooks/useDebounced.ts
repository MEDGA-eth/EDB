import { useEffect, useState } from 'react';

/**
 * Returns a value that lags behind `value` by `delayMs` — perfect for
 * filtering large lists in response to keystrokes without recomputing on
 * every change. The first render returns the input unchanged so initial
 * paint isn't delayed.
 */
export function useDebounced<T>(value: T, delayMs: number): T {
  const [debounced, setDebounced] = useState<T>(value);
  useEffect(() => {
    if (delayMs <= 0) {
      setDebounced(value);
      return;
    }
    const t = setTimeout(() => setDebounced(value), delayMs);
    return () => clearTimeout(t);
  }, [value, delayMs]);
  return debounced;
}
