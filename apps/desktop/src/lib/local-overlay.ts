import type { PluginManifest } from '@hysilens/plugin';
import { useEffect, useSyncExternalStore } from 'react';

/**
 * 原生 overlay 窗口的位置与尺寸，单位为逻辑像素
 */
export interface OverlayGeometry {
  /**
   * 外层窗口左边缘在虚拟桌面坐标系中的位置；在主显示器左侧的显示器上为负值
   */
  x: number;

  /**
   * 外层窗口上边缘在虚拟桌面坐标系中的位置；在主显示器上方的显示器上为负值
   */
  y: number;

  /**
   * 逻辑内宽，由原生 host 限制在 160–8192
   */
  width: number;

  /**
   * 逻辑内高，由原生 host 限制在 80–8192
   */
  height: number;
}

/**
 * 原生 overlay 窗口的持久化偏好，与 Tauri host 共享
 */
export interface LocalOverlaySettings {
  /**
   * overlay 窗口是否显示
   */
  enabled: boolean;

  /**
   * 鼠标输入是否穿透 overlay 落到下方桌面
   */
  clickThrough: boolean;

  /**
   * 最近一次已知的窗口位置，下次显示 overlay 时恢复
   */
  geometry: OverlayGeometry;
}

interface DesktopBridge {
  /**
   * 从原生 host 读取持久化的 overlay 偏好
   */
  getOverlay(): Promise<LocalOverlaySettings>;

  /**
   * 持久化两个开关；窗口 geometry 由 host 保留 overlay 上报的值
   */
  setOverlay(
    settings: Pick<LocalOverlaySettings, 'enabled' | 'clickThrough'>,
  ): Promise<LocalOverlaySettings>;
}

declare global {
  interface Window {
    readonly __HYSILENS_DESKTOP__?: DesktopBridge;
  }
}

/**
 * 读取原生 bridge，前提是当前文档是 host 的主 frame
 *
 * 原生 host 只把它注入主 frame，浏览器和无头客户端没有 bridge
 * 因此不会渲染仅桌面端可用的控件
 */
export function getDesktopBridge(): DesktopBridge | undefined {
  return window.__HYSILENS_DESKTOP__;
}

function subscribe(listener: () => void): () => void {
  window.addEventListener('hysilens:desktop-ready', listener);

  return () => window.removeEventListener('hysilens:desktop-ready', listener);
}

/**
 * 订阅启动后才出现的原生 bridge
 *
 * host 注入 bridge 之前返回 `undefined`，在桌面 app 之外则始终返回 `undefined`
 */
export function useDesktopBridge(): DesktopBridge | undefined {
  return useSyncExternalStore(subscribe, getDesktopBridge, () => undefined);
}

/**
 * 把 plugin 期望的 viewport 告诉原生 host
 *
 * 立即发送尺寸，并在 overlay 每次上报就绪时再次发送；preview 模式或 manifest
 * 仍在加载时不做任何事，默认回退到 960×320 逻辑像素
 */
export function useOverlayViewport(plugin: PluginManifest | undefined, preview: boolean) {
  const width = plugin?.viewport?.width ?? 960;
  const height = plugin?.viewport?.height ?? 320;
  const ready = Boolean(plugin);

  useEffect(() => {
    if (preview || !ready) {
      return;
    }

    const resize = () =>
      window.postMessage(
        {
          kind: 'hysilens:local-overlay',
          action: 'size',
          width,
          height,
        },
        location.origin,
      );

    window.addEventListener('hysilens:overlay-ready', resize);
    resize();

    return () => window.removeEventListener('hysilens:overlay-ready', resize);
  }, [preview, ready, width, height]);
}
