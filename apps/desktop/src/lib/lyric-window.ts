import type { LyricLine } from '@hysilens/protocol';

/**
 * 歌词位置处应高亮的行索引
 *
 * 返回 `startMs` 小于等于 `position` 的最后一行，因此间隙和空的定时行会保持前一行
 * 活跃，`position` 为 `null` 或早于首行开始时返回 -1
 */
export function focusLine(lines: LyricLine[], position: number | null): number {
  if (position === null) {
    return -1;
  }

  let index = -1;

  for (let next = 0; next < lines.length && lines[next].startMs <= position; next++) {
    index = next;
  }

  return index;
}

/**
 * renderer 是否可以从 `previous` 动画过渡到 `current`，而不是直接跳变
 *
 * 滑动要求只前进一行，且前后相邻行的开始时间都已知、间隔小于 1000 ms
 * 即两行连续播放，seek、跳过的行以及缺失的相邻行（`null`）都直接跳变
 */
export function canSlide(
  previous: number,
  current: number,
  before: number | null,
  after: number | null,
): boolean {
  return (
    current === previous + 1 &&
    before !== null &&
    after !== null &&
    after >= before &&
    after - before < 1000
  );
}
