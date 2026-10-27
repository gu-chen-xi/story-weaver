// 插件系统入口（v3.5.7 起是**再导出壳**）。
//
// 真正的代码在 `src/plugins/`：
//   types.ts / config.ts   契约与配置键
//   PluginCard.tsx         通用卡片（只排版，不认 id）
//   registry.tsx           注册表（加插件只改这里）
//   builtin/<id>/          具体插件：manifest（元信息）+ panel（界面）
//
// 为什么保留这个文件：仓库里有 20 多处 `from "./plugins"` / `"../plugins"` 的 import
// （视图、hooks、App）。先做成再导出壳，可以让"拆文件"与"改调用方"分两次提交，
// 每次都能跑通测试 —— 比一次性大改安全。
//
// ⚠ 新代码请直接从具体模块 import：
//     import type { PluginDescriptor } from "./plugins/types";
//     import { PluginCard } from "./plugins/PluginCard";
//     import { BUILTIN_PLUGINS } from "./plugins/registry";

export type {
  PluginCapabilities,
  PluginDescriptor,
  PluginModule,
  PluginStatus,
  PluginState,
  PluginTheme,
} from "./plugins/types";
export { pluginCaps } from "./plugins/types";

export { pluginConfigKey, LEGACY_PLUGIN_KEYS, pluginKeyOf } from "./plugins/config";

export type { PluginCardTokens } from "./plugins/PluginCard";
export { PluginCard } from "./plugins/PluginCard";

export type {
  CloudForm,
  GenDescriptor,
  GenProbe,
  GenResult,
  GenSupports,
  PortraitGenPanelProps,
} from "./plugins/builtin/portrait-gen/panel";
export { DEFAULT_NEGATIVE, PortraitGenPanel, characterToPrompt } from "./plugins/builtin/portrait-gen/panel";

export { BUILTIN_PLUGINS, pluginModuleOf } from "./plugins/registry";
export { PORTRAIT_GEN_MODULE } from "./plugins/builtin/portrait-gen";
export type { HostPayload, PluginHostData } from "./plugins/host";
