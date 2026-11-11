/** Host-side scaling applies equally to DOM, SVG, Canvas and WebGL plugins. */
export function pluginScale(value: unknown, supported: boolean): number {
  if (!supported || typeof value !== 'number' || !Number.isFinite(value)) {
    return 1;
  }

  return Math.max(0.25, Math.min(3, value));
}
