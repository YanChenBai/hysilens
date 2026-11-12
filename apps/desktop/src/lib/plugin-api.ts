import type { PluginCatalog, PluginConfig } from './api';

/**
 * 仅供歌词渲染页面和 preview iframe 读取 plugin 元数据；不提供控制操作
 */
async function read<T>(path: string): Promise<T> {
  const response = await fetch(path, { cache: 'no-store' });

  if (!response.ok) {
    throw new Error(await response.text());
  }

  return response.json() as Promise<T>;
}

/**
 * OBS、歌词窗口和 preview 使用的只读资源 client
 */
export const pluginApi = {
  listPlugins: () => read<PluginCatalog>('/api/plugins'),
  getPluginConfig: (id: string) =>
    read<PluginConfig>(`/api/plugins/${encodeURIComponent(id)}/config`),
};
