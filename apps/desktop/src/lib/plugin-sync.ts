import type { QueryClient } from '@tanstack/react-query';
import { useQueryClient } from '@tanstack/react-query';
import { useEffect } from 'react';

import { acquireSession, onPluginEvent } from './session';
import type { PluginEvent } from './session';

/** Map a pushed notification to the queries it makes stale. */
export function invalidatePluginQueries(queries: QueryClient, event: PluginEvent): void {
  switch (event.kind) {
    case 'plugins': {
      void queries.invalidateQueries({ queryKey: ['plugins'] });
      break;
    }

    case 'pluginConfig': {
      void queries.invalidateQueries({ queryKey: ['plugin-config', event.id] });
      void queries.invalidateQueries({ queryKey: ['config-editor', event.id] });
      break;
    }

    case 'resync': {
      void queries.invalidateQueries({ queryKey: ['plugins'] });
      void queries.invalidateQueries({ queryKey: ['plugin-config'] });
      void queries.invalidateQueries({ queryKey: ['config-editor'] });
      break;
    }
  }
}

/**
 * Keep plugin queries current from server pushes instead of polling. Mount once near the root;
 * it holds the event socket open but does not re-render on playback updates.
 */
export function usePluginSync(): void {
  const queries = useQueryClient();

  useEffect(() => {
    const release = acquireSession();
    const unsubscribe = onPluginEvent(event => invalidatePluginQueries(queries, event));

    return () => {
      unsubscribe();
      release();
    };
  }, [queries]);
}
