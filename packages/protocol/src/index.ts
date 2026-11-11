export type PlaybackSource = 'auto' | 'wesing' | 'qqMusic' | 'kugou';
import type { OverlayOptions } from './config';

export const PROTOCOL_VERSION = 1;

export type ClientMessage =
  | { kind: 'overlay'; settings: OverlayOptions }
  | { kind: 'source'; value: PlaybackSource }
  | { kind: 'clock'; clientMs: number }
  | { kind: 'select'; generation: number; key: string | null }
  | { kind: 'offset'; value: number }
  | { kind: 'display'; romanization: boolean; translation: boolean };

export type Session = {
  sourceId: string;
  title: string;
  artist: string;
  album: string;
  status: string;
  positionMs: number | null;
  durationMs: number | null;
  sourcePositionMs: number | null;
  sourceUpdatedUnixMs: number | null;
  rate: number | null;
  rateBasis: string;
  error: string | null;
};

export type { LyricLine, LyricDocument } from '@hysilens/plugin';
import type { LyricDocument } from '@hysilens/plugin';

export type CacheEntry = {
  key: string;
  filename: string;
  title: string;
  artist: string;
  album: string;
  lineCount: number;
};

export type Snapshot = {
  overlay?: OverlayOptions;
  source: PlaybackSource;
  version: number;
  epoch: string;
  seq: number;
  generation: number;
  playback: { serverMs: number; sessions: Session[]; error: string | null };
  session: Session | null;
  lyricsStatus: string;
  candidates: CacheEntry[];
  document: LyricDocument | null;
  offsetMs: number;
  showRomanization: boolean;
  showTranslation: boolean;
  cacheIssues: string[];
};

export type ServerMessage =
  | { kind: 'state'; state: Omit<Snapshot, 'document'> & { document?: LyricDocument | null } }
  | { kind: 'clock'; clientMs: number; serverMs: number }
  | { kind: 'error'; message: string }
  /** The plugin catalog changed; refetch `/api/plugins`. */
  | { kind: 'plugins'; revision: number }
  /** A plugin's saved config changed; refetch `/api/plugins/{id}/config`. */
  | { kind: 'pluginConfig'; id: string };

export interface AppSettings {
  version: number;
  source: PlaybackSource;
  port: number;
  cacheDir: string;
  kugouCacheDir: string;
  overlay: OverlayOptions;
}
