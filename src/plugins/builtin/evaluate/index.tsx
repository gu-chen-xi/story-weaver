// 「评估模型」插件：元信息 + 面板适配器 + 卡片上的徽标与手动运行。
//
// 它把宿主机给的 `hostData` 映射成 `EvalPanel` 需要的 props ——
// 这一步只有"知道 evaluate 长什么样"的代码才写得出来，所以它必须待在这里，
// 而不是散在 PluginsPage 的 if 里。

import type { PluginModule } from "../../types";
import { EVALUATE_MANIFEST } from "./manifest";
import { EvalPanel } from "./panel";

export const EVALUATE_MODULE: PluginModule = {
  descriptor: EVALUATE_MANIFEST,

  renderPanel: ({ T, evaluate }) => <EvalPanel T={T} {...evaluate} />,

  badges: ({ state, host }) => {
    const out: { text: string; tone: "ok" | "muted" | "warn" }[] = [
      state.enabled
        ? { text: "已开启 · 每轮自动评估", tone: "ok" }
        : { text: "已关闭", tone: "muted" },
    ];
    const n = host.evaluate.records.length;
    if (n > 0) out.push({ text: `已评 ${n} 条`, tone: "muted" });
    if (!host.hasStory) out.push({ text: "需进入故事", tone: "warn" });
    return out;
  },

  run: ({ host }) => ({
    label: "评估本节点最后一条回复",
    hint: host.hasStory ? undefined : "请先进入一个故事",
    busy: host.evaluate.busy,
    canRun: host.hasStory && host.hasNode,
    onClick: host.evaluate.onRun,
  }),

  toggleMessage: (on) =>
    on
      ? "✅ 评估模型：已开启每轮自动评估（会多调一次 API）"
      : "✅ 评估模型：已关闭自动评估（仍可手动评估与查看）",
};
