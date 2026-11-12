import { MAX_EXTRAPOLATION_MS } from './protocol';
import type { ClockAnchor } from './protocol';

/**
 * 至少达到该幅度的修正视为 seek 或切歌，立即生效
 */
export const CLOCK_SNAP_MS = 400;

/**
 * 更小的修正会在这段窗口内线性吸收
 * 它比 `CLOCK_SNAP_MS` 更长，因此按速率 1 播放时显示时间不会倒退
 */
export const CLOCK_SLEW_MS = 600;

/**
 * 把在 `receivedAt` 收到的 anchor 投影到 `now`，并做上界限制，使 host 失联时时间冻结
 */
export function project(anchor: ClockAnchor, receivedAt: number, now: number): number | null {
  if (anchor.position === null) {
    return null;
  }

  const elapsed = Math.min(MAX_EXTRAPOLATION_MS, Math.max(0, now - receivedAt));

  return anchor.position + elapsed * anchor.rate;
}

/**
 * 带平滑的单调歌词时间视图，由 `createClock` 创建
 */
export interface Clock {
  /**
   * 送入最新的权威 anchor；小修正做 slew，大修正直接 snap
   */
  readonly set: (anchor: ClockAnchor) => void;

  /**
   * 当前显示时间，单位 ms；anchor 没有位置时返回 `null`
   */
  readonly time: () => number | null;
}

/**
 * plugin 看到的歌词时间
 * 采样得到的播放位置会有几十毫秒的抖动，因此小修正采用 slew 渐入，
 * 而不是在逐字动画里表现为可见跳变
 */
export function createClock(initial: ClockAnchor, now: () => number): Clock {
  let anchor = initial;
  let receivedAt = now();
  /* 最近一次修正到达时显示时间与权威时间之差，会衰减到 0 */
  let error = 0;
  let slewStart = receivedAt;

  function time(): number | null {
    const at = now();
    const target = project(anchor, receivedAt, at);

    if (target === null || error === 0) {
      return target;
    }

    const remaining = 1 - Math.min(1, (at - slewStart) / CLOCK_SLEW_MS);

    return target + error * remaining;
  }

  return {
    set: next => {
      const displayed = time();
      anchor = next;
      receivedAt = now();
      slewStart = receivedAt;

      const target = project(anchor, receivedAt, receivedAt);
      const delta = displayed === null || target === null ? Infinity : displayed - target;

      /* 暂停、位置未知或正在 seek：精确显示权威位置 */
      error = next.rate === 0 || Math.abs(delta) >= CLOCK_SNAP_MS ? 0 : delta;
    },
    time,
  };
}
