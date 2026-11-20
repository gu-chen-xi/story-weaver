// 双池对比的漂移护栏（2026-11-19 重写）。
//
// ## 原来错在哪
//
// 2026-11-14 的护栏拿"最近一次真实请求的 `tail_block` **总字数**"，去比预览输出的
// "**三块子集**"（关系强度 / 角色属性 / 示例对话）的字数，容差 20% 或 300 字。
// 这两个数根本不是一种东西：真实尾部里还有当前模式、篇幅、数值段全量、世界书、
// 关键记忆、聚焦、约束、导演指令…… 故事一大，子集必然远小于全量 →
// **每次打开都报红**（用户实测：真实 1273 字 vs 预览 459 / 264，差 814）。
//
// ## 现在怎么判
//
// 改判"**预览里的这一行，在最近一次真实请求的尾部里存不存在**"：
//   - 预览与真实装配**同源**（2026-11-19 起 `tail_view.rs` 是唯一实现），所以预览多出来的行
//     只可能来自两种情况：两边的"输入"不同（这一句 `@了谁`、面板焦点，预览拿不到），
//     或者有人又抄了一份选择逻辑；
//   - 逐行 `includes` 对照，命中就是一致；有缺失行才报，并把缺失的行直接显示出来（可定位）。
//
// 取舍：这是**单向**检查（预览 ⊆ 真实）。真实尾部比预览多出来的行不判（那里面本来就有
// 当前模式 / 约束等预览不负责的块），代价是"真实装配多给了一行、预览少给一行"这种漂移抓不到 ——
// 那种情况只能靠 `tail_view_tests` 那组单测兜底。

export type DriftReport = {
  /// 一致返回 true
  ok: boolean;
  /// 在真实尾部里找不到的预览行（去空白后）
  missing: string[];
  /// 参与对照的预览行数
  checked: number;
  /// 命中最多的那一栏（用来告诉用户"最接近的是哪边"）
  side: "global" | "role";
  /// 真实尾部总字数（仅用于显示）
  realChars: number;
};

const linesOf = (text: string): string[] =>
  text
    .split("\n")
    .map((l) => l.trim())
    // 段标题是预览**自己起的名**（【数值与当下·关系强度】这种），真实尾部里叫【数值与当下】——
    // 标题不参与对照，只核对内容行
    .filter((l) => l.length > 0 && !l.startsWith("【"));

/// `real` = 最近一次真实请求的 `tail_block` 原文；两栏 = 预览拼出来的两版尾部子集。
/// 真实尾部为空（没有请求记录）→ 返回 null（不判）。
export function driftAgainst(real: string, globalText: string, roleText: string): DriftReport | null {
  if (!real.trim()) return null;
  const pick = (text: string) => {
    const lines = linesOf(text);
    const missing = lines.filter((l) => !real.includes(l));
    return { lines, missing };
  };
  const g = pick(globalText);
  const r = pick(roleText);
  // 取缺得少的那一栏：最近一次请求可能是全局视角、也可能是角色视角
  const useRole = r.missing.length < g.missing.length;
  const best = useRole ? r : g;
  return {
    ok: best.missing.length === 0,
    missing: best.missing,
    checked: best.lines.length,
    side: useRole ? "role" : "global",
    realChars: real.trim().length,
  };
}
