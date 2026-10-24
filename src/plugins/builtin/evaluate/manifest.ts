// 「评估模型」插件的元信息（v3.5.7，从 constants.ts 的 PLUGIN_REGISTRY 搬来）。
import type { PluginDescriptor } from "../../types";

export const EVALUATE_MANIFEST: PluginDescriptor = {
    id: "evaluate",
    icon: "🧠",
    name: "评估模型",
    status: "ready",
    desc: "给 AI 回复做质检：输出排版 / 角色贴合 / 接续前文 / 剧情推进 四维各 1~5 分，每维必须附证据引文。"
      + "开启后每轮回复自动评一次；关闭后仍可随时手动评估与查看历史。",
    caps: { canToggle: true, canRunManually: true, canViewPanel: true, needsStory: true },
    toggleLabel: "每轮回复后自动评估",
    toggleHint: "开启后每轮回复会多消耗一次 API 调用"
  };