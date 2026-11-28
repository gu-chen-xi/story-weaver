// 顶层常量与词表。
// 词表类（敏感词 / 标签 / 事件标签）**可以随时扩充**，逻辑不要跟着改。

import type { PluginDescriptor } from "./plugins";
import type { CharacterForm } from "./types";
import { BUILTIN_PLUGINS } from "./plugins/registry";
import { T } from "./theme";

export const EMPTY_CHAR_FORM: CharacterForm = {
  name: "", alias: "", role: "未定位", faction: "", age: "", gender: "",
  first_appearance: "", one_liner: "", profile: "", speech_style: "", traits: "",
  stat_stamina: 50, stat_sanity: 50, stat_emotion: 50, stat_screen: 50,
  // v3.4「角色卡 v2」
  background_past: "", appearance: "", motivation: "", goal: "", dialogue_examples: "", behavior_rules: "",
};

// 敏感词表已移到 `src/lib/sensitive.ts`（2026-11-01）：那里有归一化、变体护栏和单测，
// 这里不再保留副本 —— 两处各存一份迟早会不同步。

// 立绘「表情差分」的预置表情名（2026-10-25）。
// 词表可以随时扩充（加表情不用改逻辑）；用户也能在弹窗里自己填别的。
// 这里只影响**选择器里列什么**，不限制取值 —— 数据层就是立绘的 tags。
export const EXPRESSION_PRESETS = [
  "冷静", "愠怒", "微笑", "悲伤", "惊讶", "疲惫", "苦笑", "害羞", "生气", "落泪",
];

// ===== 插件注册表 =====
// 加一个插件 = 往这里加一条描述 + （可选）提供一个面板组件。卡片布局、开关、状态徽标、能力提示都由
// ./plugins 的 PluginCard 统一渲染。
//
// 能力声明（caps）决定卡片上出现哪些控件：
//   canToggle      总开关（如「三维数值自动变化」的启用开关）
//   canRunManually 可手动跑一次（如插件模板里的「运行一次」）
//   canViewPanel   有可展开的结果面板（如 Ollama 连接与配置）
//   needsStory     必须先进入一个故事
//
// 风格约定：
//   · 总开关只控制"是否自动跑"，**手动查看/执行不受开关限制**
//   · legacyKey 的兼容统一由 `src/plugins/config.ts` 维护

// v3.5.7：注册表搬到 `src/plugins/registry.tsx`（面板也进注册表）。
// 这里保留再导出，避免一次性改掉所有调用方；新代码请直接用 BUILTIN_PLUGINS。
export const PLUGIN_REGISTRY: PluginDescriptor[] = BUILTIN_PLUGINS.map((m) => m.descriptor);

/** 没有立绘时用的占位渐变（按立绘自身的 g 键取） */
export const PORTRAIT_GRADS: Record<string, string> = {
  g1: "linear-gradient(160deg,#4a5f8a,#22293a)",
  g2: "linear-gradient(160deg,#7b5cff,#3a2b6b)",
  g3: "linear-gradient(160deg,#c9584f,#5a2521)",
  g4: "linear-gradient(160deg,#3f8f6d,#1e3d30)",
  g5: "linear-gradient(160deg,#d0913c,#5a3d17)",
  g6: "linear-gradient(160deg,#4f8fb8,#22404f)",
};

// ===== 提示词分区配色（sectionColor 用） =====

export const SECTION_COLORS: Record<string, string> = {
  "基础指令": T.info,
  "角色": T.purple,
  "关系": T.purpleSoft,
  "世界观": T.ok,
  "当前模式": T.warn,
  "本次聚焦": T.warn,
  "剧情约束": T.danger,
  "篇幅要求": T.info,
  "时空提示": T.teal,
};

export const REL_STATES = ["稳定", "紧张", "破裂"] as const;

export const REL_STATE_HINT: Record<string, string> = {
  "稳定": "冲突止于拌嘴",
  "紧张": "容易升级为争执",
  "破裂": "出现回避、断绝、伤害性言行",
};
export const DIRECTOR_TAG = "〔导演指令〕";

export const DIRECTOR_TAG_RE = /^\s*〔导演指令〕/;
export const EVENT_LABEL: Record<string, string> = {
  confession: "表白", acceptance: "接受", rejection: "拒绝", care: "关心", conflict: "冲突",
  promise: "承诺", sacrifice: "守护", betrayal: "伤害",
  cooldown_skip: "冷却跳过", validation_reject: "校验拒绝",
};
