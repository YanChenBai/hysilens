import { lazy, Suspense } from 'react';

import { PluginView } from './components/PluginView';
import { usePluginSync } from './lib/plugin-sync';

const Control = lazy(() => import('./pages/Control'));

/**
 * 按 pathname 在控制页面与 plugin overlay 之间路由
 */
export default function App() {
  usePluginSync();

  if (['/control', '/settings'].includes(location.pathname)) {
    return (
      <Suspense fallback={null}>
        <Control />
      </Suspense>
    );
  }

  return <PluginView playground={location.pathname === '/playground'} />;
}
