interface RowBounds {
  /**
   * 行相对 track 顶部的偏移，单位 CSS 像素
   */
  top: number;

  /**
   * 测量得到的行高，单位 CSS 像素
   */
  height: number;
}

/**
 * 让当前行保持在视口中点，歌曲首尾也一样
 *
 * 播放开始前或还没到第一行时 `focused` 为 `-1`；缺失的相邻行留空位，不把当前行拉离中心
 */
export function centeredWindow(
  rows: RowBounds[],
  focused: number,
  count: number,
  activeScale: number,
) {
  if (!rows.length) {
    return null;
  }

  const anchor = Math.max(0, Math.min(focused, rows.length - 1));
  const before = Math.floor((count - 1) / 2);
  const after = count - before - 1;
  const start = Math.max(0, anchor - before);
  const end = Math.min(rows.length, anchor + after + 1);

  const active = rows[anchor];
  const center = active.top + active.height / 2;
  const above = center - rows[start].top + Math.max(0, before - anchor) * active.height;
  const last = rows[end - 1];

  const below =
    last.top + last.height - center + Math.max(0, anchor + after + 1 - rows.length) * active.height;

  const halfHeight = Math.max(above, below, (active.height * activeScale) / 2);

  return { start, end, height: halfHeight * 2, translateY: halfHeight - center };
}
