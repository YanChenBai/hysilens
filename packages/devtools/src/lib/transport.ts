import type { ClockAnchor, LyricLine } from '@hysilens/plugin';

/**
 * 模拟播放器：位置为 `anchor` 加上自 `anchorAt` 起按 `rate` 流逝的时间
 *
 * 所有位置和时长都以歌词毫秒为单位；`anchorAt` 是 `performance.now()` 时间戳
 */
export interface Transport {
  playing: boolean;

  /**
   * 播放速度倍率；1 表示实时
   */
  rate: number;

  loop: boolean;

  /**
   * 已加载 sample 的长度
   */
  durationMs: number;

  /**
   * `anchorAt` 时刻的歌词位置
   */
  anchor: number;

  /**
   * 记录当前 anchor 的时间戳
   */
  anchorAt: number;
}

export function createTransport(durationMs: number, now: number): Transport {
  return { playing: true, rate: 1, loop: true, durationMs, anchor: 0, anchorAt: now };
}

const clamp = (value: number, max: number) => Math.min(max, Math.max(0, value));

export function positionAt(transport: Transport, now: number): number {
  if (!transport.playing) {
    return transport.anchor;
  }

  const raw = transport.anchor + (now - transport.anchorAt) * transport.rate;

  if (raw < transport.durationMs) {
    return raw;
  }

  return transport.loop ? raw % transport.durationMs : transport.durationMs;
}

/**
 * 未开启 loop 时播放超过结尾，对 plugin clock 而言视为停止
 */
export function isRunning(transport: Transport, now: number): boolean {
  return transport.playing && (transport.loop || positionAt(transport, now) < transport.durationMs);
}

function rebase(transport: Transport, now: number): Transport {
  return { ...transport, anchor: positionAt(transport, now), anchorAt: now };
}

export function seek(transport: Transport, position: number, now: number): Transport {
  return { ...transport, anchor: clamp(position, transport.durationMs), anchorAt: now };
}

export function setPlaying(transport: Transport, playing: boolean, now: number): Transport {
  const next = rebase(transport, now);

  /* 在非 loop 播放的末尾按下播放会从头开始，与媒体播放器一致 */
  if (playing && next.anchor >= next.durationMs) {
    return { ...next, playing, anchor: 0 };
  }

  return { ...next, playing };
}

export function setRate(transport: Transport, rate: number, now: number): Transport {
  return { ...rebase(transport, now), rate };
}

export function setLoop(transport: Transport, loop: boolean, now: number): Transport {
  return { ...rebase(transport, now), loop };
}

/**
 * 加载新的 sample：回到开头，保留当前播放状态、速度和 loop 设置
 */
export function load(transport: Transport, durationMs: number, now: number): Transport {
  return { ...transport, durationMs, anchor: 0, anchorAt: now };
}

export function clockAnchor(transport: Transport, now: number): ClockAnchor {
  return {
    position: positionAt(transport, now),
    rate: isRunning(transport, now) ? transport.rate : 0,
  };
}

export function lineIndexAt(lines: LyricLine[], time: number): number {
  return lines.findLastIndex(line => line.startMs <= time);
}

/**
 * 类似播放器的上一曲按钮：重新开始当前行，除非刚过该行开头
 */
export function previousLineStart(lines: LyricLine[], time: number): number {
  const index = lineIndexAt(lines, time);
  const current = lines[index];

  /* 第一行之前没有更早的位置可去，只能回到最开头 */
  if (!current) {
    return 0;
  }

  if (time - current.startMs > 1000) {
    return current.startMs;
  }

  return lines[index - 1]?.startMs ?? 0;
}

export function nextLineStart(lines: LyricLine[], time: number): number | null {
  return lines[lineIndexAt(lines, time) + 1]?.startMs ?? null;
}

export function formatTime(ms: number): string {
  const total = Math.max(0, Math.floor(ms));
  const minutes = Math.floor(total / 60_000);
  const seconds = Math.floor((total % 60_000) / 1000);
  const millis = total % 1000;

  return `${minutes}:${String(seconds).padStart(2, '0')}.${String(millis).padStart(3, '0')}`;
}
