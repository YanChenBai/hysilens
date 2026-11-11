import type { LyricDocument, LyricLine, Session, Snapshot } from '@hysilens/protocol';
import type { OverlayOptions } from '@hysilens/protocol/config';

function normalize(value: string): string {
  return value.normalize('NFKC').trim().replace(/\s+/gu, ' ').toLowerCase();
}

/** Metadata survives cache renames and lyric re-fetches; incomplete metadata uses content ID. */
export function lyricSongId(document: LyricDocument, session: Session | null): string {
  const title = normalize(session?.title || document.metadata.title);
  const artist = normalize(session?.artist || document.metadata.artist);
  const album = normalize(session?.album || document.metadata.album);

  return title && artist
    ? `song:v1:${JSON.stringify([title, artist, album])}`
    : `document:${document.id}`;
}

/** Match content and timestamp so a changed lyric version cannot hide an unrelated row. */
export function lyricLineId(line: LyricLine): string {
  return JSON.stringify([line.startMs, normalize(line.text)]);
}

interface IgnoredLyrics {
  document: LyricDocument;
  intervals: Array<{ start: number; end: number }>;
}

const cache = new WeakMap<LyricDocument, { signature: string; result: IgnoredLyrics }>();

export function ignoredLyricState(snapshot: Snapshot | null, overlay: OverlayOptions) {
  const document = snapshot?.document;

  if (!document) {
    return { document: null };
  }

  const selected = overlay.ignoredLyricLines[lyricSongId(document, snapshot.session)] ?? [];
  const filtered = ignoreLyrics(document, selected);

  return {
    document: filtered.document,
  };
}

export function ignoreLyrics(document: LyricDocument, selected: string[]): IgnoredLyrics {
  const signature = JSON.stringify(selected);
  const previous = cache.get(document);

  if (previous?.signature === signature) {
    return previous.result;
  }

  const ignored = new Set(selected);
  const original = document.tracks.find(track => track.kind === 'original')?.lines ?? [];

  const intervals = original.flatMap((line, index) =>
    ignored.has(lyricLineId(line))
      ? [{ start: line.startMs, end: original[index + 1]?.startMs ?? Infinity }]
      : [],
  );

  // Remove selected spans rather than reserving blank rows in scrolling renderers.
  // Resolve ends against the unfiltered track so timed renderers retain real gaps.
  const filtered = intervals.length
    ? {
        ...document,
        tracks: document.tracks.map(track => ({
          ...track,
          lines: track.lines.flatMap((line, index) => {
            const end = line.endMs ?? track.lines[index + 1]?.startMs ?? Infinity;

            const boundaries = intervals
              .flatMap(range => [range.start, range.end])
              .filter(time => time > line.startMs && time < end);

            const points = [...new Set([line.startMs, ...boundaries, end])].sort((a, b) => a - b);

            return points.slice(0, -1).flatMap((start, part) => {
              const finish = points[part + 1]!;
              const hidden = intervals.some(range => start >= range.start && start < range.end);

              if (hidden) {
                return [];
              }

              return [
                {
                  ...line,
                  startMs: start,
                  endMs: Number.isFinite(finish) ? finish : null,
                  tokens: line.tokens.filter(
                    token => token.endMs > start && token.startMs < finish,
                  ),
                },
              ];
            });
          }),
        })),
      }
    : document;

  const result = { document: filtered, intervals };
  cache.set(document, { signature, result });

  return result;
}
