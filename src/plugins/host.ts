// 宿主机交给插件面板的**统一数据**（v3.5.7）。
//
// 为什么需要这一层：
//   各插件的面板 props 完全不同（评估模型要记录、立绘生成要生成器列表…）。
//   如果注册表里直接放 `ComponentType`，宿主机就必须按 id 分支去拼 props ——
//   那正是这次重构要消灭的东西。
//
// 所以宿主机只交**一份 hostData**（下面这个形状），
// 由**插件自己**在 `index.tsx` 里把它映射成自己的面板 props。
// id 相关的知识因此留在 `builtin/<id>/` 里，宿主机零分支、零断言。

import type { EvalPanelProps } from "./builtin/evaluate/panel";
import type { PortraitGenPanelProps } from "./builtin/portrait-gen/panel";
import type { PluginCardTokens } from "./PluginCard";

/** 面板 props 里除了主题之外的那部分（宿主负责填） */
export type HostPayload<T> = Omit<T, "T">;

export type PluginHostData = {
  /** 主题对象（面板统一从 hostData 取，不再各自 import theme） */
  T: PluginCardTokens;
  /** 「评估模型」用到的数据 */
  evaluate: HostPayload<EvalPanelProps>;
  /** 「立绘生成」用到的数据 */
  portraitGen: HostPayload<PortraitGenPanelProps>;
};
