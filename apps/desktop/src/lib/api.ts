import type { PluginManifest } from '@hysilens/plugin';
import type { AnySchemaObject, ValidateFunction } from 'ajv';
import Ajv2020 from 'ajv/dist/2020';

/**
 * plugin registry 提供的目录扫描结果
 */
export interface PluginCatalog {
  /**
   * 校验通过且已安装的 plugin
   */
  plugins: PluginManifest[];

  /**
   * 校验或加载失败的 plugin 对应的可读问题描述
   */
  errors: string[];

  /**
   * 服务端上报的 catalog 版本号，存在时才有
   */
  revision?: number;
}

/**
 * plugin 的 schema 以及当前已存储的 config 值
 */
export interface PluginConfig {
  /**
   * plugin 为其 config 声明的 JSON Schema
   */
  schema: AnySchemaObject;

  /**
   * 服务端返回的已存储 theme/config 值
   */
  config: Record<string, unknown>;
}

/**
 * 按 schema 属性名索引的 plugin config 值
 */
export type ConfigValues = Record<string, unknown>;

/*
 * `x-hysilens-*` 注解属于 UI 元数据，因此未知关键字不能让编译失败
 * `allErrors` 让一次校验找出所有非法字段，而不是遇到第一个就停
 */
const lenient = new Ajv2020({
  strict: false,
  useDefaults: true,
  removeAdditional: true,
  allErrors: true,
});

const exact = new Ajv2020({ strict: false, useDefaults: true, allErrors: true });
const compiled = new Map<string, ValidateFunction>();

function validator(ajv: Ajv2020, mode: string, plugin: PluginManifest): ValidateFunction {
  const key = `${mode}:${plugin.id}:${plugin.revision}`;
  let validate = compiled.get(key);

  if (!validate) {
    validate = ajv.compile(plugin.configSchema as AnySchemaObject);
    compiled.set(key, validate);
  }

  return validate;
}

function asObject(value: unknown): ConfigValues {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
    ? structuredClone(value as ConfigValues)
    : {};
}

/**
 * 交给运行中 plugin 的配置：未知键会被丢弃，非法值回退到 schema 默认值
 * 因此过期的配置文件不会让 overlay 变成空白
 */
export function loadConfig(plugin: PluginManifest, value: unknown): ConfigValues {
  const validate = validator(lenient, 'load', plugin);
  const config = asObject(value);

  if (validate(config)) {
    return config;
  }

  for (const error of validate.errors ?? []) {
    /* 顶层 JSON Pointer 片段，按 RFC 6901 反转义 */
    const field = error.instancePath.split('/')[1]?.replaceAll('~1', '/').replaceAll('~0', '~');

    if (field) {
      delete config[field];
    }
  }

  if (!validate(config)) {
    throw new Error(lenient.errorsText(validate.errors, { dataVar: '插件配置' }));
  }

  return config;
}

/**
 * 即将保存的配置：按输入原样上报，并补全默认值
 */
export function checkConfig(plugin: PluginManifest, value: unknown): ConfigValues {
  const validate = validator(exact, 'save', plugin);
  const config = asObject(value);

  if (!validate(config)) {
    throw new Error(exact.errorsText(validate.errors, { dataVar: '插件配置' }));
  }

  return config;
}
