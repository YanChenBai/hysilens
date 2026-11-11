/** Wire contract between a Hysilens host page and a plugin iframe. */

export const API_VERSION = 1;

/** Sent once through `window.postMessage`; the transferred port carries everything else. */
export const CONNECT_KIND = 'hysilens:connect';

/** Posted by the plugin page once it listens, so the host can connect even after `load`. */
export const WAITING_KIND = 'hysilens:waiting';

/** Namespace reserved for plugins shipped with the app; external plugins cannot use it. */
export const BUILTIN_PREFIX = 'builtin:';

/** `<name>` for external plugins, `builtin:<name>` for bundled ones. */
export const PLUGIN_ID_PATTERN = /^(?:builtin:)?[a-zA-Z0-9_-]{1,80}$/;

export function isBuiltinId(id: string): boolean {
  return id.startsWith(BUILTIN_PREFIX);
}

/** Folder and file stem for an id; `:` is not a valid path character on Windows. */
export function localName(id: string): string {
  return isBuiltinId(id) ? id.slice(BUILTIN_PREFIX.length) : id;
}

/** Extrapolate a clock anchor no further than this, matching the host's own playback model. */
export const MAX_EXTRAPOLATION_MS = 2000;

export interface LyricToken {
  text: string;
  startMs: number;
  endMs: number;
}

export interface LyricLine {
  startMs: number;
  endMs: number | null;
  text: string;
  tokens: LyricToken[];
}

export interface LyricTrack {
  kind: string;
  lines: LyricLine[];
}

export interface LyricDocument {
  id: string;
  filename: string;
  metadata: { title: string; artist: string; album: string };
  tracks: LyricTrack[];
  warnings: string[];
}

export interface Playback {
  playing: boolean;
  /** Host decision combining connection health and session state. */
  visible: boolean;
}

/** Lyric time at the moment of sending, offset already applied; `rate` is 0 while paused. */
export interface ClockAnchor {
  position: number | null;
  rate: number;
}

export interface InitPayload {
  config: Record<string, unknown>;
  lyrics: LyricDocument | null;
  playback: Playback;
  clock: ClockAnchor;
  preview: boolean;
}

export type HostMessage =
  | ({ kind: 'init' } & InitPayload)
  | { kind: 'config'; config: Record<string, unknown> }
  | { kind: 'lyrics'; document: LyricDocument | null }
  | ({ kind: 'playback' } & Playback)
  | ({ kind: 'clock' } & ClockAnchor)
  | { kind: 'dispose' };

export type PluginMessage =
  | { kind: 'ready' }
  | { kind: 'error'; message: string; fatal: boolean }
  | { kind: 'disposed' };

export interface ConnectMessage {
  kind: typeof CONNECT_KIND;
  apiVersion: typeof API_VERSION;
}

export interface PluginViewport {
  width: number;
  height: number;
}

export interface PluginFeatures {
  scale?: boolean;
  font?: boolean | { families?: string[]; default?: string };
  reveal?: boolean;
  romanization?: boolean;
  translation?: boolean;
}

/** `dist/hysilens.json`, the only file a host reads to discover a plugin. */
export interface PluginManifest {
  apiVersion: typeof API_VERSION;
  id: string;
  name: string;
  version: string;
  description?: string;
  entry: string;
  styles: string[];
  viewport?: PluginViewport;
  features: PluginFeatures;
  configSchema: Record<string, unknown>;
  revision: string;
}

export function isConnectMessage(value: unknown): value is ConnectMessage {
  return (
    typeof value === 'object' &&
    value !== null &&
    (value as ConnectMessage).kind === CONNECT_KIND &&
    (value as ConnectMessage).apiVersion === API_VERSION
  );
}

export function errorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message || error.name;
  }

  return typeof error === 'string' ? error : (JSON.stringify(error) ?? String(error));
}
