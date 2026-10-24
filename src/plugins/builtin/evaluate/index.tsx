// 「评估模型」插件：元信息 + 面板适配器。
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
};
