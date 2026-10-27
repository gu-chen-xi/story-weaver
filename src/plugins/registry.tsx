// 插件注册表（v3.5.7）。
//
// **这是"加一个插件"唯一需要改的文件。**
//
// 加插件三步：
//   1. 在 `src/plugins/builtin/<id>/` 建目录：
//        manifest.ts   元信息（id / 名称 / 状态 / 能力声明）
//        panel.tsx     界面（可选）
//        index.tsx     导出 PluginModule（把 hostData 适配成自己的 props）
//   2. 在本文件 import 它的 MODULE，往 `BUILTIN_PLUGINS` 里加一条
//   3. 需要专属配置键（历史遗留）→ 在 `plugins/config.ts` 的 LEGACY_PLUGIN_KEYS 补一行
//
// 不需要改：`PluginCard`（通用排版）、`PluginsPage`（遍历注册表）、`usePlugins`（开关逻辑）。

import type { PluginModule } from "./types";
import { PORTRAIT_GEN_MODULE } from "./builtin/portrait-gen";

/** 「规划中」的占位插件：只有元信息，没有面板 */
const OLLAMA_PLACEHOLDER: PluginModule = {
  descriptor: {
    id: "ollama",
    icon: "🦙",
    name: "Ollama 本地模型",
    status: "planned",
    desc: "接入本地大模型，实现纯离线剧情生成。",
    plannedNote: "依赖插件系统（模型列表发现、下拉选择接入哪个本地模型），后置。",
  },
};

/**
 * 内置插件清单。顺序 = 界面上卡片的顺序。
 * 规划中的插件也放进来自成一卡（面板为空，只展示"为什么还没做"）。
 */
export const BUILTIN_PLUGINS: PluginModule[] = [
  OLLAMA_PLACEHOLDER,
  PORTRAIT_GEN_MODULE,
];

/** 按 id 取一条 */
export function pluginModuleOf(id: string): PluginModule | undefined {
  return BUILTIN_PLUGINS.find((m) => m.descriptor.id === id);
}
