// 行级 diff（2026-11-14）：双池对比里"到底改了啥"就靠它。
//
// 为什么自己写：只要行级增删，不需要字符级高亮，也不想引入依赖。
// LCS 的 O(n·m) 在提示词这个量级（几百行）完全够用；超过上限就退化成"整段替换"，
// 免得极端情况下把页面卡住。

export type DiffLine = { kind: "same" | "add" | "del"; text: string };

const MAX_LINES = 1200;

/** 按行比较 before → after；返回按顺序排列的 same / add / del */
export function diffLines(before: string, after: string): DiffLine[] {
  const a = before.split("\n");
  const b = after.split("\n");
  if (a.length > MAX_LINES || b.length > MAX_LINES) {
    // 太大：不逐行比（会慢），直接给"全删 + 全加"，让调用方知道超限
    return [
      ...a.map((text) => ({ kind: "del" as const, text })),
      ...b.map((text) => ({ kind: "add" as const, text })),
    ];
  }
  // DP：lcs[i][j] = a[i..] 与 b[j..] 的最长公共子序列长度
  const n = a.length;
  const m = b.length;
  const lcs: Uint16Array[] = Array.from({ length: n + 1 }, () => new Uint16Array(m + 1));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      lcs[i][j] = a[i] === b[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
    }
  }
  const out: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      out.push({ kind: "same", text: a[i] });
      i++;
      j++;
    } else if (lcs[i + 1][j] >= lcs[i][j + 1]) {
      out.push({ kind: "del", text: a[i] });
      i++;
    } else {
      out.push({ kind: "add", text: b[j] });
      j++;
    }
  }
  while (i < n) out.push({ kind: "del", text: a[i++] });
  while (j < m) out.push({ kind: "add", text: b[j++] });
  return out;
}

/** 统计：删了多少行、加了多少行（界面上显示用） */
export function diffStats(lines: DiffLine[]): { added: number; removed: number } {
  let added = 0;
  let removed = 0;
  for (const l of lines) {
    if (l.kind === "add") added++;
    else if (l.kind === "del") removed++;
  }
  return { added, removed };
}
