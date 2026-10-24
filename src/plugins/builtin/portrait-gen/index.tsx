// 「立绘生成」插件：元信息 + 面板适配器 + 卡片徽标。

import type { PluginModule } from "../../types";
import { PORTRAIT_GEN_MANIFEST } from "./manifest";
import { PortraitGenPanel } from "./panel";

export const PORTRAIT_GEN_MODULE: PluginModule = {
  descriptor: PORTRAIT_GEN_MANIFEST,

  renderPanel: ({ T, portraitGen }) => <PortraitGenPanel T={T} {...portraitGen} />,

  badges: ({ host }) => {
    const { generators, selected } = host.portraitGen;
    const g = generators.find((x) => x.id === selected);
    if (!g) return [];
    return [g.implemented
      ? { text: g.name, tone: "ok" as const }
      : { text: `${g.name}·未实装`, tone: "warn" as const }];
  },
};
