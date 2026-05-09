import { useEffect, useRef } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { useSession } from '../store/session';
import { Health } from '../lib/types';
import { HEALTHCHECK_FAILURE_THRESHOLD, HEALTHCHECK_INTERVAL_MS } from '../lib/constants';

async function probeHealth(): Promise<boolean> {
  try {
    const res = await fetch('/health');
    if (!res.ok) return false;
    const v = await res.json();
    Health.parse(v);
    return true;
  } catch { return false; }
}

export function useHealthcheck() {
  const setConnection = useSession(s => s.setConnection);
  const setSessionEnded = useSession(s => s.setSessionEnded);
  const queryClient = useQueryClient();
  const sessionEnded = useSession(s => s.sessionEnded);

  // Track previous sessionEnded value across renders so we only react on the
  // false → true transition (preventing infinite invalidation loops).
  const prevEndedRef = useRef<boolean>(sessionEnded);

  // When sessionEnded flips false → true, cancel any in-flight queries and
  // drop their cached results so a reconnected session can't surface stale
  // data from a previous engine.
  useEffect(() => {
    if (!prevEndedRef.current && sessionEnded) {
      void queryClient.cancelQueries();
      queryClient.removeQueries();
    }
    prevEndedRef.current = sessionEnded;
  }, [sessionEnded, queryClient]);

  useEffect(() => {
    let misses = 0;
    let cancelled = false;
    let intervalId: ReturnType<typeof setInterval> | null = null;

    const tick = async () => {
      const ok = await probeHealth();
      if (cancelled) return;
      if (ok) { misses = 0; setConnection('connected'); setSessionEnded(false); }
      else {
        misses++;
        setConnection(misses === 1 ? 'degraded' : 'offline');
        if (misses >= HEALTHCHECK_FAILURE_THRESHOLD) setSessionEnded(true);
      }
    };

    const start = () => {
      if (intervalId !== null) return;
      void tick();
      intervalId = setInterval(tick, HEALTHCHECK_INTERVAL_MS);
    };
    const stop = () => {
      if (intervalId !== null) {
        clearInterval(intervalId);
        intervalId = null;
      }
    };
    // Pause polling while the tab is hidden; resume + immediate probe when
    // it returns. `document.hidden` semantics differ slightly across happy-dom
    // / browsers, so guard the access.
    const onVisibility = () => {
      if (typeof document === 'undefined') return;
      if (document.hidden) stop();
      else start();
    };

    if (typeof document === 'undefined' || !document.hidden) start();
    if (typeof document !== 'undefined') {
      document.addEventListener('visibilitychange', onVisibility);
    }

    return () => {
      cancelled = true;
      stop();
      if (typeof document !== 'undefined') {
        document.removeEventListener('visibilitychange', onVisibility);
      }
    };
  }, [setConnection, setSessionEnded]);
}
