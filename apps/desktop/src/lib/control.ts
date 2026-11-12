import type { PluginManifest } from '@hysilens/plugin';
import type { AppSettings, ClientMessage, PlaybackSource, ServerMessage } from '@hysilens/protocol';
import { Channel, invoke } from '@tauri-apps/api/core';

import type { ConfigValues, PluginCatalog, PluginConfig } from './api';
import { getDesktopBridge } from './local-overlay';

/**
 * 当前诊断平台及采集说明
 */
export interface DiagnosticInfo {
  source: PlaybackSource | null;

  name: string;

  description: string;
}

/**
 * 酷狗增强状态；portOpen 表示调试端口在本机监听
 */
export interface EnhancedStatus {
  enabled: boolean;

  port: number;

  portOpen: boolean;

  endpointAvailable: boolean;

  kugouRunning: boolean;

  connected: boolean;

  error: string | null;
}

type Operations = {
  settingsGet: [undefined, AppSettings];

  settingsSet: [{ settings: AppSettings }, void];

  fontsList: [undefined, string[]];

  pluginsList: [undefined, PluginCatalog];

  pluginsRescan: [undefined, PluginCatalog];

  pluginsOpenDirectory: [undefined, void];

  pluginsRemove: [{ id: string }, PluginCatalog];

  pluginConfigGet: [{ id: string }, PluginConfig];

  pluginConfigSet: [{ id: string; config: ConfigValues }, void];

  directoryPick: [{ directory: string }, { path: string | null }];

  diagnosticsGet: [undefined, DiagnosticInfo];

  diagnosticsExport: [{ source: PlaybackSource | null }, { path: string | null }];

  kugouSet: [{ enabled: boolean }, void];

  kugouLaunch: [undefined, { result: string }];

  kugouShortcut: [undefined, { path: string }];

  sessionSend: [{ message: ClientMessage }, ServerMessage | null];
};

/**
 * IPC channel 复用 snapshot 协议，额外报告连接和缓存重同步
 */
export type ControlEvent =
  | (ServerMessage & { sentAtMs?: number })
  | { kind: 'connection'; connected: boolean }
  | { kind: 'kugou'; status: EnhancedStatus }
  | { kind: 'resync' };

/**
 * 检查主窗口是否获得 host 的核心控制 IPC 能力
 */
export function hasControlIpc(): boolean {
  return (
    typeof window !== 'undefined' &&
    window === window.top &&
    getDesktopBridge()?.controlIpc === true
  );
}

/**
 * 控制页面只使用 IPC，浏览器中的控制页面不会降级到 web 通道
 */
export function isControlPage(): boolean {
  return ['/control', '/settings'].includes(location.pathname);
}

async function ipc<T>(command: string, args: Parameters<typeof invoke>[1]): Promise<T> {
  if (!hasControlIpc()) {
    throw new Error('请在 Hysilens 桌面主窗口中打开控制界面');
  }

  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw error instanceof Error ? error : new Error(String(error));
  }
}

function request<M extends keyof Operations>(
  method: M,
  params: Operations[M][0],
): Promise<Operations[M][1]> {
  return ipc('control_request', { request: { method, ...params } });
}

/**
 * 有明确输入和返回类型的控制 client；桌面 IPC 失败时不重试 HTTP
 */
export const control = {
  getSettings: () => request('settingsGet', undefined),
  setSettings: (settings: AppSettings) => request('settingsSet', { settings }),
  listFonts: () => request('fontsList', undefined),
  listPlugins: () => request('pluginsList', undefined),
  rescanPlugins: () => request('pluginsRescan', undefined),
  openPluginDirectory: () => request('pluginsOpenDirectory', undefined),
  removePlugin: (id: string) => request('pluginsRemove', { id }),
  getPluginConfig: (id: string) => request('pluginConfigGet', { id }),
  setPluginConfig: (id: string, config: ConfigValues) => request('pluginConfigSet', { id, config }),
  pickDirectory: (directory: string) => request('directoryPick', { directory }),
  getDiagnostics: () => request('diagnosticsGet', undefined),
  exportDiagnostics: (source: PlaybackSource | null) => request('diagnosticsExport', { source }),
  setKugou: (enabled: boolean) => request('kugouSet', { enabled }),
  launchKugou: () => request('kugouLaunch', undefined),
  createKugouShortcut: () => request('kugouShortcut', undefined),
};

/**
 * ZIP 直接作为 Tauri 二进制 IPC 请求，不保留 HTTP 上传入口
 */
export async function installPlugin(file: Blob): Promise<PluginManifest> {
  return ipc('control_install_plugin', await file.arrayBuffer());
}

/**
 * 播放控制走同一个核心 reducer；clock 回复复用 ServerMessage
 */
export function sendControl(message: ClientMessage): Promise<ServerMessage | null> {
  return ipc('control_request', { request: { method: 'sessionSend', message } });
}

/**
 * 返回同步清理函数；订阅尚在创建时卸载也会释放随后返回的 id
 */
export function subscribeControl(
  receive: (event: ControlEvent) => void,
  onError: (error: Error) => void,
): () => void {
  if (!hasControlIpc()) {
    onError(new Error('请在 Hysilens 桌面主窗口中打开控制界面'));

    return () => {};
  }

  let active = true;
  let subscription: number | undefined;

  const channel = new Channel<ControlEvent>(event => {
    if (active) {
      try {
        receive(event);
      } catch (error) {
        onError(error instanceof Error ? error : new Error(String(error)));
      }
    }
  });

  const release = (id: number) => {
    void ipc('control_unsubscribe', { id }).catch(error => {
      if (active) {
        onError(error);
      }
    });
  };

  void ipc<number>('control_subscribe', { channel })
    .then(id => {
      if (!active) {
        release(id);

        return;
      }

      subscription = id;
    })
    .catch(error => {
      if (active) {
        onError(error);
      }
    });

  return () => {
    if (!active) {
      return;
    }

    active = false;

    if (subscription !== undefined) {
      release(subscription);
    }
  };
}
