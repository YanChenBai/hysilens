import { QueryClient } from '@tanstack/react-query';
import { describe, expect, it } from 'vite-plus/test';

import { invalidatePluginQueries } from './plugin-sync';

function seeded() {
  const queries = new QueryClient();

  for (const key of [
    ['plugins'],
    ['plugin-config', 'builtin', 'r1'],
    ['plugin-config', 'tilted', 'r1'],
    ['config-editor', 'builtin', 'r1'],
    ['config-editor', 'tilted', 'r1'],
  ]) {
    queries.setQueryData(key, {});
  }

  const stale = () =>
    queries
      .getQueryCache()
      .getAll()
      .filter(query => query.isStale())
      .map(query => query.queryKey.join(':'))
      .sort();

  return { queries, stale };
}

describe('invalidatePluginQueries', () => {
  it('refreshes only the catalog when plugins change', () => {
    const { queries, stale } = seeded();

    invalidatePluginQueries(queries, { kind: 'plugins' });

    expect(stale()).toEqual(['plugins']);
  });

  it('refreshes one plugin config in both the view and the editor', () => {
    const { queries, stale } = seeded();

    invalidatePluginQueries(queries, { kind: 'pluginConfig', id: 'tilted' });

    expect(stale()).toEqual(['config-editor:tilted:r1', 'plugin-config:tilted:r1']);
  });

  it('refreshes everything after reconnecting', () => {
    const { queries, stale } = seeded();

    invalidatePluginQueries(queries, { kind: 'resync' });

    expect(stale()).toHaveLength(5);
  });
});
