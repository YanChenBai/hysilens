import { useEffect, useState } from 'react';

/**
 * `active` 为真时每个动画帧刷新一次的 `performance.now()`，仅用于时间显示
 */
export function useNow(active = true): number {
  const [now, setNow] = useState(() => performance.now());

  useEffect(() => {
    if (!active) {
      setNow(performance.now());

      return;
    }

    let frame = 0;

    const tick = () => {
      setNow(performance.now());
      frame = requestAnimationFrame(tick);
    };

    frame = requestAnimationFrame(tick);

    return () => cancelAnimationFrame(frame);
  }, [active]);

  return now;
}
