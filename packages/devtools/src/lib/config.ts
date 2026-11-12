import type { PluginFeatures, PluginViewport } from '@hysilens/plugin';

/**
 * 由 `hysilens dev` 提供；当 devtools 也由该服务器托管时为空
 */
export const target = (import.meta.env.VITE_HYSILENS_TARGET as string | undefined) ?? '';

/**
 * `@hysilens/plugin` 为单个配置字段生成的 JSON Schema 子集，
 * 外加 devtools 表单用作 UI 元数据的 `x-hysilens-*` 注解
 */
export interface SchemaField {
  type?: string;

  title?: string;

  description?: string;

  default?: unknown;

  enum?: unknown[];

  minimum?: number;

  maximum?: number;

  maxLength?: number;

  pattern?: string;

  /**
   * 为该字段渲染的表单控件，不再从 `type` 推断
   */
  'x-hysilens-control'?: 'color' | 'font' | 'slider';

  /**
   * slider 增量；缺省时由控件自行选择合适的 step
   */
  'x-hysilens-step'?: number;

  'x-hysilens-enum-labels'?: Record<string, string>;

  'x-hysilens-font-families'?: string[];
}

/**
 * dev server 在 `/__hysilens/config.json` 暴露的 `hysilens.config.ts` 子集
 */
export interface PluginInfo {
  id: string;

  name: string;

  description?: string;

  viewport?: PluginViewport;

  features: PluginFeatures;

  configSchema: { properties?: Record<string, SchemaField> };
}

export type ConfigValues = Record<string, unknown>;

export async function fetchPluginInfo(): Promise<PluginInfo> {
  const response = await fetch(`${target}/__hysilens/config.json`, { cache: 'no-store' });

  if (!response.ok) {
    throw new Error(`读取插件配置失败（${response.status}）`);
  }

  return response.json() as Promise<PluginInfo>;
}

export function fields(info: PluginInfo): Array<[string, SchemaField]> {
  return Object.entries(info.configSchema.properties ?? {});
}

export function defaults(info: PluginInfo): ConfigValues {
  return Object.fromEntries(fields(info).map(([name, field]) => [name, field.default]));
}

const storageKey = (info: PluginInfo) => `hysilens-devtools:config:${info.id}`;

/**
 * 保留 schema 中仍存在的字段的修改；新字段使用其默认值
 */
export function reconcile(info: PluginInfo, config: ConfigValues): ConfigValues {
  const base = defaults(info);

  return { ...base, ...Object.fromEntries(Object.entries(config).filter(([key]) => key in base)) };
}

/**
 * 在 schema 默认值之上叠加已保存的修改；schema 中已不存在的键会被丢弃
 */
export function restoreConfig(info: PluginInfo): ConfigValues {
  try {
    const saved = JSON.parse(sessionStorage.getItem(storageKey(info)) ?? '{}') as ConfigValues;

    return reconcile(info, saved);
  } catch {
    return defaults(info);
  }
}

export function persistConfig(info: PluginInfo, config: ConfigValues): void {
  sessionStorage.setItem(storageKey(info), JSON.stringify(config));
}
