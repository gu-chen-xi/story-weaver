// 行级 diff（给「生成参数 → Prompt 预览」的"对比两次请求"用）。
//
// 为什么自己写：项目零额外依赖（npm 装不了新包），而需求只是"看清两次提示词差在哪"，
// 不需要 Myers 那种最优算法。这里用**朴素 LCS**（O(n·m)）：
//   提示词通常几百行，n·m 最多几十万次比较，在前端主线程也是毫秒级。
//
// 只做纯函数，方便 `npm test` 直接测。

export type DiffRow =
  /** 两边都有（同位置内容相同） */
  | { kind: "same"; text: string }
  /** 只在左边（旧）有 */
  | { kind: "del"; text: string }
  /** 只在右边（新）有 */
  | { kind: "add"; text: string };

/// 对两段文本做行级 diff，返回按顺序排列的行。
///
/// 返回的是完整序列（含未变化的行）—— 界面上要显示上下文，
/// 只给"差异行"会让用户看不出改动落在哪一段。
export function diffLines(before: string, after: string): DiffRow[] {
  const a = before.split("\n");
  const b = after.split("\n");
  const n = a.length;
  const m = b.length;

  // dp[i][j] = a[i..] 与 b[j..] 的最长公共子序列长度
  const dp: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }

  const out: DiffRow[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      out.push({ kind: "same", text: a[i] });
      i++;
      j++;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      out.push({ kind: "del", text: a[i] });
      i++;
    } else {
      out.push({ kind: "add", text: b[j] });
      j++;
    }
  }
  while (i < n) {
    out.push({ kind: "del", text: a[i] });
    i++;
  }
  while (j < m) {
    out.push({ kind: "add", text: b[j] });
    j++;
  }
  return out;
}

/// 统计差异（界面顶部的"新增 x 行 / 删除 y 行"）。
export function diffStat(rows: DiffRow[]): { added: number; removed: number } {
  let added = 0;
  let removed = 0;
  for (const r of rows) {
    if (r.kind === "add") added++;
    else if (r.kind === "del") removed++;
  }
  return { added, removed };
}
