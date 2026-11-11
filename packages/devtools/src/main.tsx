import { ThemeProvider, TooltipProvider } from '@hysilens/ui';
import { createRoot } from 'react-dom/client';

import App from './App';

import './styles.css';

const root = document.getElementById('root');

if (!root) {
  throw new Error('Missing devtools root');
}

createRoot(root).render(
  <ThemeProvider storageKey="hysilens-devtools-theme" defaultTheme="dark">
    <TooltipProvider>
      <App />
    </TooltipProvider>
  </ThemeProvider>,
);
