/**
 * 公开的 plugin SDK：`definePlugin` 以及 plugin 渲染所依据的 wire 类型
 */
export { definePlugin } from './runtime';

export type { ConfigCarrier, ConfigOf, Plugin, PluginContext, PluginDefinition } from './runtime';

export type {
  ClockAnchor,
  LyricDocument,
  LyricLine,
  LyricToken,
  LyricTrack,
  Playback,
  PluginFeatures,
  PluginManifest,
  PluginViewport,
} from './protocol';
