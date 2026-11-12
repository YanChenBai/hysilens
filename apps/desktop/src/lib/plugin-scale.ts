/**
 * host 侧的缩放对 DOM、SVG、Canvas 与 WebGL plugin 同样生效
 *
 * @param value plugin 配置中的 `scale` 设置，类型未知
 * @param supported plugin 是否声明了 `scale` feature
 * @returns 限制在 0.25–3 的乘数；不支持缩放或值不是有限数字时返回 1
 */
export function pluginScale(value: unknown, supported: boolean): number {
  if (!supported || typeof value !== 'number' || !Number.isFinite(value)) {
    return 1;
  }

  return Math.max(0.25, Math.min(3, value));
}
