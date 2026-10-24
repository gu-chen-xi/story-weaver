// 示例插件的元信息（模板）。
//
// 用法：把整个 `_template/` 目录复制成 `builtin/<你的插件 id>/`，改这四件事：
//   1. 下面三个常量（id / 名称 / 图标）
//   2. 能力声明 `caps` —— 卡片上出现哪些控件由它决定
//   3. `panel.tsx` 的界面（不需要面板就删掉那个文件与 index.tsx 里的 renderPanel）
//   4. 在 `plugins/registry.tsx` 的 BUILTIN_PLUGINS 里加一行
//
// 详细说明见 `docs/dev/PLUGINS.md`。

import type { PluginDescriptor } from "../types";

/** 本插件的稳定 id —— 别处引用时用这个常量，不要写死字符串 */
export const TEMPLATE_PLUGIN_ID = "template-example";

export const TEMPLATE_MANIFEST: PluginDescriptor = {
  id: TEMPLATE_PLUGIN_ID,
  icon: "🧪",
  name: "示例插件（模板）",
  // "planned" = 占位（界面只展示 plannedNote）；"ready" = 可用
  status: "ready",
  desc: "一句话说清它给用户带来什么。也写清边界：没配置/服务不可用时，不会影响其它功能。",
  caps: {
    canToggle: true, // 有总开关吗
    canRunManually: false, // 卡片上要不要「运行一次」按钮
    canViewPanel: true, // 有没有可展开的面板
    needsStory: false, // 是否必须先进入一个故事
  },
  toggleLabel: "开启后做什么",
  toggleHint: "开启的代价（比如会多消耗一次 API 调用）—— 必须诚实写出来",
};
