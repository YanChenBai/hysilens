import { FieldError } from '@hysilens/ui';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { RotateCw } from 'kubai-icons-react';
import { useState } from 'react';

import { getJson, loadConfig, rescanPlugins } from '../lib/api';
import type { PluginCatalog, PluginConfig } from '../lib/api';
import { sendOverlay } from '../lib/session';
import { Choice } from './Choice';
import { IconButton } from './IconButton';
import { InstallPluginButton } from './InstallPluginButton';
import { OpenPluginDirectoryButton } from './OpenPluginDirectoryButton';
import { ThemeEditor } from './ThemeEditor';

/**
 * plugin 标签页：renderer 选择、安装/重新扫描操作以及当前 plugin 的主题编辑器
 *
 * @param active overlay config 中当前选中的 renderer id
 */
export function PluginPanel({ active }: { active: string }) {
  const [switchError, setSwitchError] = useState('');
  const [notice, setNotice] = useState('');
  const queries = useQueryClient();

  const catalog = useQuery({
    queryKey: ['plugins'],
    queryFn: () => getJson<PluginCatalog>('/api/plugins'),
  });

  const rescan = useMutation({
    mutationFn: rescanPlugins,
    onSuccess: next => queries.setQueryData(['plugins'], next),
  });

  const data = catalog.data ?? { plugins: [], errors: [] };
  const plugin = data.plugins.find(item => item.id === active);

  const theme = useQuery({
    queryKey: ['config-editor', plugin?.id, plugin?.revision],
    enabled: Boolean(plugin),
    queryFn: async (): Promise<PluginConfig> => {
      if (!plugin) {
        throw new Error('未找到插件');
      }

      const value = await getJson<PluginConfig>(`/api/plugins/${plugin.id}/config`);

      return { ...value, config: loadConfig(plugin, value.config) };
    },
  });

  const failure = [catalog.error, rescan.error, theme.error].find(Boolean);
  const error = switchError || (failure ? String(failure) : '');

  return (
    <section className="flex h-full min-h-0 flex-col">
      <div className="shrink-0 space-y-4 px-5 pb-5">
        <div className="flex items-center justify-between gap-4">
          <div>
            <h2 className="font-semibold">歌词插件</h2>
          </div>
          <div className="flex items-center gap-1">
            <InstallPluginButton
              onInstalled={manifest => {
                setSwitchError('');
                setNotice(`已安装 ${manifest.name} ${manifest.version}`);
              }}
              onError={message => {
                setNotice('');
                setSwitchError(message);
              }}
            />
            <OpenPluginDirectoryButton />
            <IconButton
              label="重新扫描插件"
              isLoading={rescan.isPending}
              onClick={() => rescan.mutate()}
            >
              <RotateCw color="#60a5fa" size={20} trigger="parent" />
            </IconButton>
          </div>
        </div>
        <Choice
          label="歌词插件"
          value={active}
          onChange={value => {
            try {
              sendOverlay({ renderer: value });
              setSwitchError('');
            } catch (error) {
              setSwitchError(String(error));
            }
          }}
          items={data.plugins.map(item => ({ value: item.id, label: item.name }))}
        />
        <FieldError show={Boolean(error)}>{error}</FieldError>
        {notice && !error ? (
          <p role="status" className="text-xs text-emerald-600 dark:text-emerald-400">
            {notice}
          </p>
        ) : null}
        {data.errors.map(error => (
          <p key={error} className="text-destructive text-xs">
            {error}
          </p>
        ))}
      </div>
      {plugin && theme.data ? (
        <ThemeEditor key={`${plugin.id}:${plugin.revision}`} plugin={plugin} initial={theme.data} />
      ) : (
        <p className="text-muted-foreground px-5 text-sm">正在读取插件配置…</p>
      )}
    </section>
  );
}
