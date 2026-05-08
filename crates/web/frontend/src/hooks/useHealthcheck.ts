import { useEffect } from 'react';
import { useSession } from '../store/session';
import { Health } from '../lib/types';
import { rpcRaw } from '../lib/rpc';

const POLL_MS = 2000;
const FAILURE_THRESHOLD = 3;

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

  useEffect(() => {
    let misses = 0;
    let cancelled = false;
    const tick = async () => {
      const ok = await probeHealth();
      if (cancelled) return;
      if (ok) { misses = 0; setConnection('connected'); setSessionEnded(false); }
      else {
        misses++;
        setConnection(misses === 1 ? 'degraded' : 'offline');
        if (misses >= FAILURE_THRESHOLD) setSessionEnded(true);
      }
    };
    void tick();
    const id = setInterval(tick, POLL_MS);
    return () => { cancelled = true; clearInterval(id); };
  }, [setConnection, setSessionEnded]);
  // exported so we can suppress in tests / compute outside React if needed
  void rpcRaw; void useSession;
}
