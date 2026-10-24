// 「立绘生成」插件：元信息 + 面板适配器。

import type { PluginModule } from "../../types";
import { PORTRAIT_GEN_MANIFEST } from "./manifest";
import { PortraitGenPanel } from "./panel";

export const PORTRAIT_GEN_MODULE: PluginModule = {
  descriptor: PORTRAIT_GEN_MANIFEST,
  renderPanel: ({ T, portraitGen }) => <PortraitGenPanel T={T} {...portraitGen} />,
};
