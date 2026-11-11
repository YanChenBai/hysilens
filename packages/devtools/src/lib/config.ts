import type { PluginFeatures, PluginViewport } from '@hysilens/plugin';

/** Served by `hysilens dev`; empty when devtools are hosted by that same server. */
export const target = (import.meta.env.VITE_HYSILENS_TARGET as string | undefined) ?? '';

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
  'x-hysilens-control'?: 'color' | 'font' | 'slider';
  'x-hysilens-step'?: number;
  'x-hysilens-enum-labels'?: Record<string, string>;
  'x-hysilens-font-families'?: string[];
}

/** The subset of `hysilens.config.ts` the dev server exposes at `/__hysilens/config.json`. */
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

/** Keep edits for fields the schema still has; new fields start at their defaults. */
export function reconcile(info: PluginInfo, config: ConfigValues): ConfigValues {
  const base = defaults(info);

  return { ...base, ...Object.fromEntries(Object.entries(config).filter(([key]) => key in base)) };
}

/** Saved edits over schema defaults; keys the schema no longer has are dropped. */
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
