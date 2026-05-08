import { useEffect, useState } from 'react';
import { IDELayout } from './layout/IDELayout';
import { MobileLayout } from './layout/MobileLayout';
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

  return (
    <ErrorBoundary label="App">
      <div className="h-screen bg-(--color-bg-root)">
        {wide ? <IDELayout /> : <MobileLayout />}
        <SessionEndedOverlay />
      </div>
    </ErrorBoundary>
  );
}
