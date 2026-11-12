import { definePlugin } from '@hysilens/plugin';
import type { ConfigOf, LyricDocument, LyricLine } from '@hysilens/plugin';
import { useSyncExternalStore } from 'react';
import { createRoot } from 'react-dom/client';

import type config from '../hysilens.config';

import './style.css';

type Config = ConfigOf<typeof config>;

interface Snapshot {
  config: Config;

  lyrics: LyricDocument | null;

  /**
   * host 希望显示歌词时为 true；暂停或空闲时为 false
   */
  visible: boolean;

  /**
   * 已应用 offset 的歌词时间
   */
  time: number | null;
}

/**
 * `useSyncExternalStore` 读取的 snapshot store；host 通过 `subscribe` 推送更新
 */
interface Store {
  subscribe: (listener: () => void) => () => void;

  get: () => Snapshot;
}

/**
 * 准备渲染的一行歌词，包含它在视口中占用的高度
 */
interface RenderLine {
  line: LyricLine;

  index: number;

  romanization?: string;

  translation?: string;

  /**
   * 行高，单位 CSS 像素：原文行加上附加轨道
   */
  height: number;
}

/**
 * 当前行的进度，按 CSS 百分比返回，限制在 `0%`–`100%`
 */
function progress(start: number, end: number, position: number): string {
  return `${Math.min(100, Math.max(0, ((position - start) / Math.max(1, end - start)) * 100))}%`;
}

/**
 * 覆盖 `time` 的附加轨道文本，没有时返回 `undefined`
 */
function secondary(lyrics: LyricDocument | null, kind: string, time: number) {
  return lyrics?.tracks
    .find(track => track.kind === kind)
    ?.lines.find(line => line.startMs <= time && (line.endMs === null || line.endMs > time))?.text;
}

function Renderer({ store }: { store: Store }) {
  const current = useSyncExternalStore(store.subscribe, store.get);

  if (!current.visible || current.time === null) {
    return null;
  }

  return <Lyrics config={current.config} lyrics={current.lyrics} time={current.time} />;
}

function Lyrics({
  config: theme,
  lyrics,
  time,
}: {
  config: Config;

  lyrics: LyricDocument | null;

  time: number;
}) {
  const lines = lyrics?.tracks.find(track => track.kind === 'original')?.lines ?? [];
  const focus = lines.findLastIndex(line => line.startMs <= time);
  const count = theme.visibleLines;

  const rows: RenderLine[] = lines.map((line, index) => {
    const romanization = theme.romanization
      ? secondary(lyrics, 'romanization', line.startMs)
      : undefined;

    const translation = theme.translation
      ? secondary(lyrics, 'translation', line.startMs)
      : undefined;

    const secondaryCount =
      Number(Boolean(romanization?.trim())) + Number(Boolean(translation?.trim()));

    return {
      line,
      index,
      romanization,
      translation,

      height: theme.fontSize * 1.3 + theme.lineGap + secondaryCount * 26,
    };
  });

  const start = Math.max(0, focus - Math.floor((count - 1) / 2));
  const visibleRows = rows.slice(start, start + count);
  const viewportHeight = visibleRows.reduce((height, row) => height + row.height, 0);
  const verticalOffset = rows.slice(0, start).reduce((height, row) => height + row.height, 0);

  return (
    <main
      className="lyrics"
      style={
        {
          color: theme.color,

          fontFamily: `${JSON.stringify(theme.fontFamily === 'ZCOOLQingKeHuangYou' ? 'Noto Sans SC' : theme.fontFamily)}, 'Noto Sans SC', sans-serif`,

          '--color': theme.color,

          '--active': theme.activeColor,

          '--font-size': `${theme.fontSize}px`,
        } as React.CSSProperties
      }
    >
      <div className="viewport" style={{ height: viewportHeight }}>
        <div className="track" style={{ transform: `translateY(-${verticalOffset}px)` }}>
          {rows.map(row => (
            <LyricRow
              key={`${lyrics?.id}:${row.index}`}
              wordHighlight={theme.wordHighlight}
              row={row}
              active={row.index === focus}
              time={time}
            />
          ))}
        </div>
      </div>
    </main>
  );
}

function LyricRow({
  wordHighlight,
  row,
  active,
  time,
}: {
  wordHighlight: boolean;

  row: RenderLine;

  active: boolean;

  time: number;
}) {
  const { line, romanization, translation } = row;

  return (
    <section
      className={`line ${active ? 'current' : ''}`}
      aria-label={active ? '当前句' : undefined}
      style={{ height: row.height }}
    >
      <div className="original">
        {wordHighlight && active && line.tokens.length ? (
          line.tokens.map((token, tokenIndex) => (
            <span
              className="token"
              key={tokenIndex}
              style={
                { '--progress': progress(token.startMs, token.endMs, time) } as React.CSSProperties
              }
            >
              {token.text}
            </span>
          ))
        ) : (
          <span className={active ? 'active' : undefined}>{line.text}</span>
        )}
      </div>
      {romanization?.trim() && <p className="secondary">{romanization}</p>}
      {translation?.trim() && <p className="secondary">{translation}</p>}
    </section>
  );
}

export default definePlugin<typeof config>({
  setup({ root, config: readConfig, lyrics, playback, time, onMounted, onUnmounted }) {
    const read = (): Snapshot => ({
      config: readConfig(),
      lyrics: lyrics(),
      visible: playback().visible,
      time: time(),
    });

    const listeners = new Set<() => void>();
    let snapshot = read();
    let frame = 0;

    const store: Store = {
      subscribe: listener => {
        listeners.add(listener);

        return () => listeners.delete(listener);
      },
      get: () => snapshot,
    };

    /* 时间连续推进，所以按帧刷新，和普通动画循环一致 */
    const tick = () => {
      snapshot = read();
      listeners.forEach(listener => listener());
      frame = requestAnimationFrame(tick);
    };

    const app = createRoot(root);

    onMounted(() => {
      app.render(<Renderer store={store} />);
      tick();
    });

    onUnmounted(() => {
      cancelAnimationFrame(frame);
      app.unmount();
    });
  },
});
