// 「立绘生成」插件的元信息（v3.5.7，从 constants.ts 的 PLUGIN_REGISTRY 搬来）。
import type { PluginDescriptor } from "../../types";

/** 本插件的稳定 id（被 hooks/usePlugins.ts 等地方引用，避免写死字符串） */
export const PORTRAIT_GEN_ID = "portrait-gen";

export const PORTRAIT_GEN_MANIFEST: PluginDescriptor = {
    id: "portrait-gen",
    icon: "🎨",
    name: "立绘生成（云端 / 本地双通道）",
    status: "ready",
    desc: "从提示词生成立绘，结果直接进素材库（走与手工导入完全相同的入口，自动记录版本与来源）。"
      + "没有可用服务时本插件只提示原因，不影响素材库的手工导入、绑定、分组、版本、导出与角色卡展示。",
    caps: { canToggle: false, canRunManually: false, canViewPanel: true }
  };