import type { LyricLine } from '@hysilens/plugin';
import { Eye, EyeOff, Pause, Play, Repeat, SkipBack, SkipForward } from 'kubai-icons-react';

import {
  formatTime,
  isRunning,
  nextLineStart,
  positionAt,
  previousLineStart,
  seek,
  setLoop,
  setPlaying,
  setRate,
} from '../lib/transport';
import type { Transport } from '../lib/transport';
import { useNow } from '../lib/use-now';
import { Choice } from './Choice';
import { IconButton } from './IconButton';

const RATES = ['0.25', '0.5', '0.75', '1', '1.25', '1.5', '2'];

/**
 * 模拟播放器的播放控制
 */
interface TransportBarProps {
  /**
   * 播放器状态；位置和时长以歌词毫秒为单位
   */
  transport: Transport;

  onTransport: (next: Transport) => void;

  /**
   * 所选 sample 的歌词行，用于上一行/下一行跳转
   */
  lines: LyricLine[];

  /**
   * 作为 `playback.visible` 传给 plugin 的值，由眼睛按钮切换
   */
  visible: boolean;

  onVisible: (visible: boolean) => void;
}

export function TransportBar({
  transport,
  onTransport,
  lines,
  visible,
  onVisible,
}: TransportBarProps) {
  const now = useNow(transport.playing);
  const position = positionAt(transport, now);
  const running = isRunning(transport, now);
  const next = nextLineStart(lines, position);
  const at = () => performance.now();

  return (
    <section
      aria-label="播放控制"
      className="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-2 rounded-xl border px-3 py-2"
    >
      <div className="flex items-center gap-1">
        <IconButton
          label="上一句"
          onClick={() => onTransport(seek(transport, previousLineStart(lines, position), at()))}
        >
          <SkipBack color="#60a5fa" size={20} trigger="parent" />
        </IconButton>
        <IconButton
          label={running ? '暂停' : '播放'}
          onClick={() => onTransport(setPlaying(transport, !running, at()))}
        >
          {running ? (
            <Pause color="#f59e0b" size={22} trigger="parent" />
          ) : (
            <Play color="#34d399" size={22} trigger="parent" />
          )}
        </IconButton>
        <IconButton
          label="下一句"
          disabled={next === null}
          onClick={() => next !== null && onTransport(seek(transport, next, at()))}
        >
          <SkipForward color="#60a5fa" size={20} trigger="parent" />
        </IconButton>
      </div>

      <div className="flex min-w-60 flex-1 items-center gap-3">
        <span className="text-muted-foreground w-20 text-right font-mono text-xs tabular-nums">
          {formatTime(position)}
        </span>
        <input
          type="range"
          aria-label="播放进度"
          className="seek h-2 min-w-0 flex-1 cursor-pointer"
          min={0}
          max={transport.durationMs}
          step={10}
          value={Math.round(position)}
          onChange={event => onTransport(seek(transport, Number(event.target.value), at()))}
        />
        <span className="text-muted-foreground w-20 font-mono text-xs tabular-nums">
          {formatTime(transport.durationMs)}
        </span>
      </div>

      <div className="flex items-center gap-1">
        <Choice
          label="播放速度"
          className="h-9 w-24"
          value={String(transport.rate)}
          items={RATES.map(rate => ({ value: rate, label: `${rate}×` }))}
          onChange={rate => onTransport(setRate(transport, Number(rate), at()))}
        />
        <IconButton
          label={transport.loop ? '循环播放：开' : '循环播放：关'}
          pressed={transport.loop}
          onClick={() => onTransport(setLoop(transport, !transport.loop, at()))}
        >
          <Repeat color={transport.loop ? '#a78bfa' : '#94a3b8'} size={20} trigger="parent" />
        </IconButton>
        <IconButton
          label={visible ? '模拟隐藏歌词（playback.visible = false）' : '恢复显示歌词'}
          pressed={!visible}
          onClick={() => onVisible(!visible)}
        >
          {visible ? (
            <Eye color="#22d3ee" size={20} trigger="parent" />
          ) : (
            <EyeOff color="#f87171" size={20} trigger="parent" />
          )}
        </IconButton>
      </div>
    </section>
  );
}
