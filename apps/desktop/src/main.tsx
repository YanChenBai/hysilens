import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createRoot } from 'react-dom/client';

import App from './App';

import './styles.css';

const control = ['/control', '/settings'].includes(location.pathname);
const previewTheme = new URLSearchParams(location.search).get('preview');
document.documentElement.classList.toggle('overlay', !control);
document.documentElement.classList.toggle(
  'preview',
  !control && ['1', 'light', 'dark', 'system'].includes(previewTheme ?? ''),
);
document.documentElement.dataset.previewTheme = previewTheme ?? '';

const queries = new QueryClient({
  defaultOptions: { queries: { retry: 1, refetchOnWindowFocus: false } },
});

const root = document.getElementById('root');

if (!root) {
  throw new Error('Missing application root');
}

createRoot(root).render(
  <QueryClientProvider client={queries}>
    <App />
  </QueryClientProvider>,
);
