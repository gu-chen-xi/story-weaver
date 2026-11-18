// 正文里的 `**加粗**` / `*斜体*` 拆分（2026-11-18）。
//
// 为什么单独一个纯函数：JSX 不认 Markdown，记忆卡正文里既有软件写的 `**已被接受**`，
// 也有 AI 自己写的 `*嘴一张就是那三个字…*`。拆 token 是纯逻辑，能被 `node:test` 直接跑；
// 渲染（`lib/rich.tsx::renderRich`）只负责把 token 变成 JSX。
//
// **宁可少渲染，也不吞字**：标记没配对时整段原样输出（包含那个 `*`），
// 因为正文是模型/作者原话，把孤立的 `*` 当成斜体起点会把字吃掉。

export type RichToken = {
  kind: "text" | "bold" | "italic";
  text: string;
};

/// 用 `mark` 切；**成对**（分隔符个数为偶数）才返回分段，否则返回 null。
function splitPaired(s: string, mark: string): string[] | null {
  const parts = s.split(mark);
  return parts.length % 2 === 1 ? parts : null;
}

export function richTokens(s: string): RichToken[] {
  if (!s) return [];
  const bold = splitPaired(s, "**");
  if (!bold) {
    // 有孤立的 `**`：整段原样，不猜
    return [{ kind: "text", text: s }];
  }
  const out: RichToken[] = [];
  bold.forEach((seg, i) => {
    if (!seg) return;
    if (i % 2 === 1) {
      out.push({ kind: "bold", text: seg });
      return;
    }
    const italic = splitPaired(seg, "*");
    if (!italic) {
      out.push({ kind: "text", text: seg });
      return;
    }
    italic.forEach((t, j) => {
      if (!t) return;
      out.push({ kind: j % 2 === 1 ? "italic" : "text", text: t });
    });
  });
  return out;
}
