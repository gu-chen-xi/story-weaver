// 纯渲染辅助（不含任何状态）。
//
// 从 App.tsx 抽出（2026-10-17）：`renderBold` 只依赖主题色，没有任何状态，
// 却因为定义在组件内而在多处被 props 传递；抽出来后各页面直接 import 即可。

import type { ReactNode } from "react";
import { T } from "../theme";

/// 把数据里的 **加粗** 标记渲染出来（JSX 不认 markdown，只能自己拆）。
export function renderBold(s: string): ReactNode[] {
  const parts = s.split("**");
  return parts.map((seg, i) =>
    i % 2 === 1
      ? <b key={i} style={{ color: T.text1 }}>{seg}</b>
      : <span key={i}>{seg}</span>
  );
}
