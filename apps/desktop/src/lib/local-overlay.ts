import type { PluginManifest } from '@hysilens/plugin';
import { useEffect, useSyncExternalStore } from 'react';

export interface OverlayGeometry {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface LocalOverlaySettings {
  enabled: boolean;
  clickThrough: boolean;
  geometry: OverlayGeometry;
}

interface DesktopBridge {
  getOverlay(): Promise<LocalOverlaySettings>;
  setOverlay(
    settings: Pick<LocalOverlaySettings, 'enabled' | 'clickThrough'>,
  ): Promise<LocalOverlaySettings>;
}

declare global {
  interface Window {
    readonly __HYSILENS_DESKTOP__?: DesktopBridge;
  }
}

// The native host injects this only into its main frame. Browsers and headless
// clients have no bridge and therefore do not render desktop-only controls.
export function getDesktopBridge(): DesktopBridge | undefined {
  return window.__HYSILENS_DESKTOP__;
}

function subscribe(listener: () => void): () => void {
  window.addEventListener('hysilens:desktop-ready', listener);

  return () => window.removeEventListener('hysilens:desktop-ready', listener);
}

export function useDesktopBridge(): DesktopBridge | undefined {
  return useSyncExternalStore(subscribe, getDesktopBridge, () => undefined);
}

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
