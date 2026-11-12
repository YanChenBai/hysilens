import type { LyricDocument, LyricLine, Session, Snapshot } from '@hysilens/protocol';
import type { OverlayOptions } from '@hysilens/protocol/config';

function normalize(value: string): string {
  return value.normalize('NFKC').trim().replace(/\s+/gu, ' ').toLowerCase();
}

/**
 * 元数据在缓存改名与重新获取歌词后仍然保留；元数据不完整时改用内容 ID
 */
export function lyricSongId(document: LyricDocument, session: Session | null): string {
  const title = normalize(session?.title || document.metadata.title);
  const artist = normalize(session?.artist || document.metadata.artist);
  const album = normalize(session?.album || document.metadata.album);

  return title && artist
    ? `song:v1:${JSON.stringify([title, artist, album])}`
    : `document:${document.id}`;
}

/**
 * 同时匹配内容与时间戳，让变更后的歌词版本无法隐藏无关的行
 */
export function lyricLineId(line: LyricLine): string {
  return JSON.stringify([line.startMs, normalize(line.text)]);
}

interface IgnoredLyrics {
  document: LyricDocument;

  /**
   * 被移除的区间，单位为毫秒，按原始行顺序排列
   */
  intervals: Array<{ start: number; end: number }>;
}

const cache = new WeakMap<LyricDocument, { signature: string; result: IgnoredLyrics }>();

/**
 * 用本歌曲中被忽略的行过滤 snapshot 的 document
 *
 * 选择未变化时从每 document 的缓存返回，让重复的 frame 保持相同的 document 标识
 *
 * @returns 过滤后的 document，snapshot 没有 document 时返回 `{ document: null }`
 */
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

/**
 * 从所有 track 中移除选中的歌词行
 *
 * 选中的 id 是来自 `lyricLineId` 的行 id；只有时间戳和文本都匹配的行才会命中
 * 因此编辑过的内容或新的歌词版本无法隐藏无关文本，次要 track 上的行会按被移除
 * 的区间切分，以保留真实的时间间隙
 *
 * @returns 过滤后的 document 与被移除的区间；输入相同时返回缓存的 document 实例
 */
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

  /*
   * 直接移除选中的区间，而不是在滚动 renderer 中预留空行
   * 结束时间基于未过滤的 track 计算，让带时间的 renderer 保留真实间隙
   */
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
