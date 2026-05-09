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
if (rootEl) {
  createRoot(rootEl).render(
    <StrictMode>
      <QueryClientProvider client={queryClient}>
        <App />
      </QueryClientProvider>
    </StrictMode>,
  );
} else {
  // eslint-disable-next-line no-console
  console.error('edb-web: #root element not found; mounting skipped.');
}
