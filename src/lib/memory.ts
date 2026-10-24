/// 关键记忆正文的显示清洗（v3.5.1）。
///
/// 为什么需要：记忆文件是 Markdown，正文形如
///
/// ```text
/// # 第 4 天 · 表白
///
/// [夏婉琴]“我爱你，跟我在一起吧。”（她把这句话在心里练了三年）
///
/// > 由系统从剧情里提炼（命中「我爱你」）。这是这个分支里第一次发生，后续同类事件不再单独记录。
/// ```
///
/// 但面板上方**已经显示**了「表白 · 第 4 天」—— 再把标题渲染一遍就是重复；
/// 而且按纯文本渲染会把 `#` 和 `>` 直接露出来。所以这里把正文拆成
/// 「描述」+「系统备注」两部分，界面分别用正常字号和小字显示。

export type MemoryBodyView = {
  /// 正文（去掉标题与引用块），可能为空
  text: string;
  /// 系统备注（原来的 `> …` 行），可能为空
  note: string;
};

export function memoryBodyView(body: string): MemoryBodyView {
  const textLines: string[] = [];
  const noteLines: string[] = [];
  for (const raw of (body || "").replace(/\r\n/g, "\n").split("\n")) {
    const line = raw.trim();
    if (!line) continue;
    if (line.startsWith("#")) continue;          // 标题：上面已经显示了标签与日期
    if (line.startsWith(">")) {                  // 引用块：系统备注
      const v = line.replace(/^>\s*/, "").trim();
      if (v) noteLines.push(v);
      continue;
    }
    textLines.push(line);
  }
  return { text: textLines.join("\n"), note: noteLines.join("\n") };
}
