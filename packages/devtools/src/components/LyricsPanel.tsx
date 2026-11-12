import type { LyricDocument, LyricLine } from '@hysilens/plugin';
import type { LyricSample } from '@hysilens/plugin/samples';
import { cn } from 'cn';
import { useEffect, useRef } from 'react';

import { formatTime, lineIndexAt, positionAt, seek } from '../lib/transport';
import type { Transport } from '../lib/transport';
import { useNow } from '../lib/use-now';
import { Choice } from './Choice';

/**
 * 与 `line` 起始时间相同的次要轨道文本，没有时返回 undefined
 */
function secondary(document: LyricDocument, kind: string, line: LyricLine) {
  return document.tracks
    .find(track => track.kind === kind)
    ?.lines.find(candidate => candidate.startMs === line.startMs)?.text;
}

/**
 * sample 选择器加歌词列表，播放头所在行会高亮
 */
interface LyricsPanelProps {
  samples: LyricSample[];

  sample: LyricSample;

  onSample: (id: string) => void;

  /**
   * 播放器状态；位置以歌词毫秒为单位
   */
  transport: Transport;

  onTransport: (next: Transport) => void;
}

export function LyricsPanel({
  samples,
  sample,
  onSample,
  transport,
  onTransport,
}: LyricsPanelProps) {
  const now = useNow(transport.playing);
  const document = sample.document;
  const lines = document.tracks.find(track => track.kind === 'original')?.lines ?? [];

  const current = lineIndexAt(lines, positionAt(transport, now));
  const list = useRef<HTMLOListElement>(null);

  useEffect(() => {
    list.current
      ?.querySelector(`[data-index="${current}"]`)
      ?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
  }, [current]);

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3">
      <div className="space-y-2 px-4">
        <Choice
          label="测试歌词"
          value={sample.id}
          items={samples.map(item => ({ value: item.id, label: item.name }))}
          onChange={onSample}
        />
        <p className="text-muted-foreground text-xs">{sample.description}</p>
      </div>
      <ol ref={list} className="min-h-0 flex-1 space-y-1 overflow-y-auto px-2 pb-4">
        {lines.map((line, index) => {
          const romanization = secondary(document, 'romanization', line);
          const translation = secondary(document, 'translation', line);

          return (
            <li key={`${line.startMs}:${index}`} data-index={index}>
              <button
                type="button"
                aria-current={index === current ? 'true' : undefined}
                className={cn(
                  'hover:bg-muted w-full rounded-lg px-3 py-2 text-left transition-colors',
                  index === current && 'bg-muted ring-ring/40 ring-1',
                )}
                onClick={() => onTransport(seek(transport, line.startMs, performance.now()))}
              >
                <span className="text-muted-foreground mr-2 font-mono text-[11px] tabular-nums">
                  {formatTime(line.startMs)}
                </span>
                <span className={cn('text-sm', !line.text && 'text-muted-foreground italic')}>
                  {line.text || '（空行）'}
                </span>
                {line.tokens.length ? null : (
                  <span className="text-muted-foreground ml-2 text-[11px]">整行</span>
                )}
                {romanization ? (
                  <span className="text-muted-foreground block text-xs">{romanization}</span>
                ) : null}
                {translation ? (
                  <span className="text-muted-foreground block text-xs">{translation}</span>
                ) : null}
              </button>
            </li>
          );
        })}
      </ol>
    </div>
  );
}
