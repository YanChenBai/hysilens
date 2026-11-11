interface RowBounds {
  top: number;
  height: number;
}

/** Keep the focused row at the viewport midpoint, including at either end of a song. */
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
  // Missing neighbours occupy blank slots instead of pulling the current line off centre.
  const above = center - rows[start].top + Math.max(0, before - anchor) * active.height;
  const last = rows[end - 1];

  const below =
    last.top + last.height - center + Math.max(0, anchor + after + 1 - rows.length) * active.height;

  const halfHeight = Math.max(above, below, (active.height * activeScale) / 2);

  return { start, end, height: halfHeight * 2, translateY: halfHeight - center };
}
