import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import App from './App';
import './styles/tailwind.css';

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: Infinity,
      // Cap memory growth: queries unobserved for 30 minutes get evicted from
      // the cache, so long time-travel sessions don't grow unboundedly.
      gcTime: 30 * 60 * 1000,
      retry: false,
      refetchOnWindowFocus: false,
    },
  },
});

const rootEl = document.getElementById('root');

function FileProtocolError() {
  return (
    <div
      data-testid="file-protocol-error"
      style={{
        fontFamily: 'system-ui, -apple-system, sans-serif',
        maxWidth: 640,
        margin: '15vh auto',
        padding: '24px',
        lineHeight: 1.5,
      }}
    >
      <h1 style={{ fontSize: 18, marginBottom: 12 }}>edb-web cannot run from a local file</h1>
      <p>
        This page was opened with the <code>file://</code> protocol, so it
        cannot reach the engine over JSON-RPC. Run the UI from the engine
        instead:
      </p>
      <pre
        style={{
          background: '#f4f4f4',
          padding: '12px',
          borderRadius: 6,
          overflowX: 'auto',
        }}
      >
        edb replay --ui=web &lt;tx&gt;
      </pre>
      <p>
        The engine binds <code>http://127.0.0.1</code> on a chosen port and
        serves both the UI and the API there.
      </p>
    </div>
  );
}

if (rootEl) {
  if (typeof window !== 'undefined' && window.location.protocol === 'file:') {
    // No SPA, fetches to '/' would be useless, and most browsers also
    // refuse XHR cross-origin under file://.
    createRoot(rootEl).render(<FileProtocolError />);
  } else {
    createRoot(rootEl).render(
      <StrictMode>
        <QueryClientProvider client={queryClient}>
          <App />
        </QueryClientProvider>
      </StrictMode>,
    );
  }
} else {
  // eslint-disable-next-line no-console
  console.error('edb-web: #root element not found; mounting skipped.');
}
