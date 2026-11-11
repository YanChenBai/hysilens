/** Fixed geometry; font, colors and the visible line count are configured separately. */
export const preset = {
  perspective: 1000,
  rotateY: -40,
  fontSize: 42,
  fontWeight: '400',
  lineGap: 32,
  activeScale: 1.4,
  inactiveOpacity: 0.52,
  strokeWidth: 1.2,
  depth: 1,
  shadow: true,
  shadowOpacity: 0.75,
  shadowBlur: 4,
  shadowX: 1,
  shadowY: 3,
} as const;
