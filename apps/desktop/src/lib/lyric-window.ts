import type { LyricLine } from '@hysilens/protocol';

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
