// 插件契约（v3.5.7，从 src/plugins.tsx 抽出）。
//
// 这一层只放**所有插件共有的东西**：元信息形状、能力声明、运行状态、配置键规则。
// 具体插件长什么样、有什么面板，放在 `plugins/builtin/<id>/` 里，通过 `registry.tsx` 组装。
//
// 能力声明（capabilities）是这套东西的关键 —— 它让"插件能做什么"变成**数据**，
// 而不是散落在 PluginsPage 的 if 里：
//   - canToggle      是否有总开关（如评估模型的「每轮自动评估」）
//   - canRunManually 是否能在界面上手动跑一次（如「评估本节点最后一条回复」）
//   - canViewPanel   是否有可展开的结果面板
//   - needsStory     是否必须先进入一个故事才能用

import type { ReactNode } from "react";
import type { PluginHostData } from "./host";

/** 插件状态：规划中（不可用）/ 已实装（可用） */
export type PluginStatus = "planned" | "ready";

/** 插件能做什么 */
export type PluginCapabilities = {
  canToggle: boolean;      // 有总开关
  canRunManually: boolean; // 可手动执行
  canViewPanel: boolean;   // 有可展开面板
  needsStory: boolean;     // 需要先进入故事
};

/** 插件元信息 */
export type PluginDescriptor = {
  id: string;              // 稳定标识，同时用作配置键前缀：plugin.<id>.enabled
  icon: string;
  name: string;
  desc: string;
  status: PluginStatus;
  /** 能力声明（可部分给出，缺省按 false） */
  caps?: Partial<PluginCapabilities>;
  /** 开关文案（有总开关时用） */
  toggleLabel?: string;
  /** 开关旁的代价说明（如"会多消耗一次 API 调用"） */
  toggleHint?: string;
  /** 规划中插件的占位说明（status = planned 时显示） */
  plannedNote?: string;
};

/** 运行期状态（由宿主维护） */
export type PluginState = {
  /** 总开关是否打开 */
  enabled: boolean;
  /** 面板是否展开 */
  panelOpen: boolean;
  /** 状态徽标（如"已开启 · 每轮自动评估"） */
  badges: { text: string; tone: "ok" | "muted" | "warn" }[];
  /** 正在执行（禁用按钮并显示忙态文案） */
  busy?: boolean;
  /** 手动执行按钮的文案 */
  runLabel?: string;
  /** 手动执行按钮的提示（如"与上面的自动开关无关"） */
  runHint?: string;
  /** 面板内容 */
  panel?: ReactNode;
};

const CAPS_DEFAULT: PluginCapabilities = {
  canToggle: false,
  canRunManually: false,
  canViewPanel: false,
  needsStory: false,
};

export function pluginCaps(d: PluginDescriptor): PluginCapabilities {
  return { ...CAPS_DEFAULT, ...(d.caps ?? {}) };
}

/**
 * 一条注册表条目 = 元信息 + **面板渲染器**（v3.5.7）。
 *
 * **面板进注册表**是这次重构的核心：PluginsPage 变成
 * `BUILTIN_PLUGINS.map(m => <PluginCard module={m} .../>)`，不再按 id 分支。
 *
 * 为什么是 `renderPanel(hostData)` 而不是直接放 `ComponentType`：
 * 各面板的 props 完全不同，直接放组件宿主就必须按 id 分支拼 props（回到老路）。
 * 交一份统一的 `hostData`、由插件自己适配，宿主机才能零分支、零断言。
 *
 * 取舍：所有面板都会被静态打进主包。对**内置插件**没问题（本来就在包里）；
 * 将来若做"外部安装的插件"，这一层要改成 `lazy()` + 动态 `import()`。
 */
export type PluginBadge = { text: string; tone: "ok" | "muted" | "warn" };

/** 卡片上「手动运行」按钮的完整描述（由插件自己给，宿主只渲染） */
export type PluginRunAction = {
  label: string;
  hint?: string;
  busy: boolean;
  canRun: boolean;
  onClick: () => void;
};

/// 插件自己的开关状态（徽标由插件自己算，所以这里不需要完整的 `PluginState`）
export type PluginToggles = { enabled: boolean; panelOpen: boolean };

/** 插件拿到的东西：它自己的开关 + 宿主机提供的处境与数据 */
export type PluginContext = { state: PluginToggles; host: PluginHostData };

export type PluginModule = {
  descriptor: PluginDescriptor;
  /** 渲染面板；`undefined` = 这个插件没有可展开界面（如"规划中"的插件） */
  renderPanel?: (data: PluginHostData) => ReactNode;
  /**
   * 卡片上的状态徽标。
   *
   * 以前这些徽标是在 `PluginsPage` 里按 `d.id === "evaluate"` 拼的 ——
   * 现在由插件自己算："已评 12 条"只有评估模型知道，"当前通道 qwen-image"只有立绘生成知道。
   */
  badges?: (ctx: PluginContext) => PluginBadge[];
  /** 卡片上的「手动运行」按钮（没有 = 不显示） */
  run?: (ctx: PluginContext) => PluginRunAction | undefined;
  /** 开关切换成功后给用户的一句提示（可选；宿主负责发出来） */
  toggleMessage?: (on: boolean) => string;
};

/** 面板用的主题对象 —— 就是 `PluginCard` 的 tokens（`src/theme.ts` 的 T 的子集形状） */
export type { PluginCardTokens as PluginTheme } from "./PluginCard";
