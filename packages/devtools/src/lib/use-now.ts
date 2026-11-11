import { useEffect, useState } from 'react';

/** `performance.now()` refreshed every animation frame while `active`; for time displays only. */
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
