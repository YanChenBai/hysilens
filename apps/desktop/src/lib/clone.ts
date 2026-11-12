/**
 * 优先原生深拷贝，旧内核通过 JSON 复制配置值；回退仅适用于 JSON 数据及 undefined
 * 不修改全局对象，原生实现抛出的错误直接交给调用方处理
 */
export function cloneValue<T>(value: T): T {
  if (typeof globalThis.structuredClone === 'function') {
    return globalThis.structuredClone(value);
  }

  if (value === undefined) {
    return value;
  }

  return JSON.parse(JSON.stringify(value)) as T;
}
