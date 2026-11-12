import type { QueryClient } from '@tanstack/react-query';
import { useQueryClient } from '@tanstack/react-query';
import { useEffect } from 'react';

import { acquireSession, onPluginEvent } from './session';
import type { PluginEvent } from './session';

/**
 * 把推送的通知映射到需要失效的 query
 *
 * @param queries 应用的 query client
 * @param event 服务端推送；`pluginConfig` 只影响该 plugin，`resync` 则刷新所有 plugin query
 */
export function invalidatePluginQueries(queries: QueryClient, event: PluginEvent): void {
  switch (event.kind) {
    case 'kugou': {
      queries.setQueryData(['kugou-enhanced'], event.status);
      break;
    }

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
      void queries.invalidateQueries({ queryKey: ['kugou-enhanced'] });
      void queries.invalidateQueries({ queryKey: ['plugins'] });
      void queries.invalidateQueries({ queryKey: ['plugin-config'] });
      void queries.invalidateQueries({ queryKey: ['config-editor'] });
      break;
    }
  }
}

/**
 * 通过服务端推送而不是轮询来保持 plugin query 最新，在根节点附近挂载一次
 * 它会保持 event socket 打开，但不会因 playback 更新而重新渲染
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
