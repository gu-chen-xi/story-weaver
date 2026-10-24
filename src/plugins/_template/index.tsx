// 示例插件的模块（模板）：把 `hostData` 适配成自己的面板 + 徽标 + 手动运行。
//
// **这个文件是"插件与宿主的唯一接口"。**
// 复制整目录后，把下面的 `count` 换成你真正要用的宿主数据即可。
//
// 注意：模板**没有**被注册（`plugins/registry.tsx` 里没有它），
// 所以它不会出现在界面上 —— 但它仍会被 `tsc` 检查，永远保持可编译。

import type { PluginModule } from "../types";
import { TEMPLATE_MANIFEST } from "./manifest";
import { TemplatePanel } from "./panel";

export const TEMPLATE_MODULE: PluginModule = {
  descriptor: TEMPLATE_MANIFEST,

  // ① 面板：从 hostData 里挑自己需要的（这里用"已评条数"当示例）
  renderPanel: ({ T, evaluate }) => <TemplatePanel T={T} count={evaluate.records.length} />,

  // ② 卡片徽标：只有你知道该显示什么
  badges: ({ state, host }) => [
    state.enabled
      ? { text: "已开启", tone: "ok" as const }
      : { text: "已关闭", tone: "muted" as const },
    ...(host.hasStory ? [] : [{ text: "需进入故事", tone: "warn" as const }]),
  ],

  // ③ 手动运行（配合 caps.canRunManually = true 才会显示）
  run: ({ host }) => ({
    label: "跑一次",
    hint: host.hasStory ? undefined : "请先进入一个故事",
    busy: false,
    canRun: host.hasStory,
    onClick: () => host.notify("示例插件：跑了一次（模板代码，不会真的做事）"),
  }),

  // ④ 开关切换后的一句提示（可选）
  toggleMessage: (on) => (on ? "✅ 示例插件：已开启" : "✅ 示例插件：已关闭"),
};
