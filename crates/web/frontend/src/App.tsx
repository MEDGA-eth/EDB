import { useEffect, useState } from 'react';
import { DesktopLayout } from './layout/DesktopLayout';
import { MobileLayout } from './layout/MobileLayout';
import { TopBar } from './components/TopBar';
import { SessionEndedOverlay } from './components/SessionEndedOverlay';
import { ErrorBoundary } from './components/ErrorBoundary';
import { useHealthcheck } from './hooks/useHealthcheck';

const DESKTOP_BREAKPOINT = 1024;

export default function App() {
  const [wide, setWide] = useState(
    () => typeof window !== 'undefined' && window.innerWidth >= DESKTOP_BREAKPOINT,
  );
  useEffect(() => {
    const onResize = () => setWide(window.innerWidth >= DESKTOP_BREAKPOINT);
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);
  useHealthcheck();

  // tx hash discovered via the engine is not currently exposed; punt for v1 —
  // TopBar accepts undefined. v2: fetch /health and extract from response payload.
  const txHash: string | undefined = undefined;

  return (
    <ErrorBoundary label="App">
      <div className="flex h-screen flex-col bg-(--color-bg-root)">
        <TopBar txHash={txHash} />
        <main className="flex-1 overflow-hidden">
          {wide ? <DesktopLayout /> : <MobileLayout />}
        </main>
        <SessionEndedOverlay />
      </div>
    </ErrorBoundary>
  );
}
