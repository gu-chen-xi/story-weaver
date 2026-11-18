// 纯渲染辅助（不含任何状态）。
//
// 从 App.tsx 抽出（2026-10-17）：`renderBold` 只依赖主题色，没有任何状态，
// 却因为定义在组件内而在多处被 props 传递；抽出来后各页面直接 import 即可。

import type { ReactNode } from "react";
import { richTokens } from "./richText";
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

/// 加粗 + 斜体都认（2026-11-18）：记忆卡正文里既有软件写的 `**已被接受**`，
/// 也有 AI 自己写的 `*…*`。拆分规则在 `lib/richText.ts`（纯函数、有单测）；
/// 这里只负责变成 JSX —— 没有 `dangerouslySetInnerHTML`，标记没配对就原样显示。
export function renderRich(s: string): ReactNode[] {
  return richTokens(s).map((t, i) => {
    if (t.kind === "bold") {
      return <b key={i} style={{ color: T.text1 }}>{t.text}</b>;
    }
    if (t.kind === "italic") {
      return <i key={i}>{t.text}</i>;
    }
    return <span key={i}>{t.text}</span>;
  });
}
