// 立绘的「表情差分」归组与切换（2026-10-25）。
//
// 背景：一个角色可以绑多张立绘（同一张脸的不同表情 / 不同差分）。导演监视器原来
// 把每张立绘都渲染成一张卡，看起来像"同一个角色出现了好几次"。现在改成一张卡 + 切换。
//
// 用户定的规则（2026-10-25 第二轮，别改）：
//   ① **切换单位是「每一张绑定的立绘」** —— 绑了 3 张就是 3 个位置（一开始按标签合并成格，
//      结果 3 张只显示出 2 格，不对）；
//   ② 标签只当**表情名**（角标文字），不参与分组：同一标签的两张仍是两个位置；
//   ③ 没有下一张 → **保持当前这张**（不回绕、不报错，避免看起来像"点了没反应"）；
//   ④ 一张立绘都没有 → **不切**（界面保持占位）。
//
// 表情名的来源：立绘上除系统标签（`AI生成` 这类）之外的第一个标签，见 `expressionTagOf`。

export type VariantPortrait = {
  id: number;
  tags: string[];
  path?: string | null;
  ver?: string;
  pinned?: boolean;
  /** 立绘上记的角色标注（监视器小卡右上角显示用）；没有就不显示 */
  role?: string;
  /** 素材名（缩略图 title 用） */
  name?: string;
  /** 占位渐变的键（没有图时用，见 constants.PORTRAIT_GRADS） */
  g?: string;
};

/** 不算「表情名」的系统标签（入库时自动打的，用户看不到也不该当表情） */
const SYSTEM_TAGS = new Set(["AI生成", "AI 生成", "AI", "ai生成"]);

/** 是不是系统标签（入库自动打的，不算表情名，改标签时要保留） */
export function isSystemTag(t: string): boolean {
  return SYSTEM_TAGS.has((t ?? "").trim());
}

/** 立绘的「表情名」：第一个非系统标签；没有则空串 */
export function expressionTagOf(p: { tags: string[] }): string {
  for (const raw of p.tags ?? []) {
    const t = (raw ?? "").trim();
    if (t && !SYSTEM_TAGS.has(t)) return t;
  }
  return "";
}

/**
 * 切成「一格一张图」。
 * 顺序 = id 升序（= 入库顺序，稳定可预期）。
 * `tag` 只用于显示（表情名），不参与合并 —— 绑 3 张就是 3 格。
 */
export function variantSlots(ps: VariantPortrait[]): { tag: string; portrait: VariantPortrait }[] {
  return [...ps]
    .sort((a, b) => a.id - b.id)
    .map((portrait) => ({ tag: expressionTagOf(portrait) || "未命名", portrait }));
}

/** 当前该显示哪张：优先钉住的，否则最新入库的那张；一张都没有 → null */
export function defaultVariant(ps: VariantPortrait[]): VariantPortrait | null {
  if (ps.length === 0) return null;
  const pinned = ps.find((p) => p.pinned && p.path);
  if (pinned) return pinned;
  return ps.reduce((a, b) => (b.id > a.id ? b : a));
}

/**
 * 切到下一张。
 * - 一张立绘都没有 → 返回 null（调用方保持占位，不动图）
 * - 只有一张 → 原样返回（**不**回绕，避免看起来像"没反应"）
 * - 当前不在列表里（比如刚换了角色）→ 给第一格
 */
export function nextVariant(ps: VariantPortrait[], currentId: number | null): VariantPortrait | null {
  const slots = variantSlots(ps);
  if (slots.length === 0) return null;
  const idx = slots.findIndex((s) => s.portrait.id === currentId);
  if (idx < 0) return slots[0].portrait;
  if (slots.length === 1) return slots[0].portrait;
  return slots[(idx + 1) % slots.length].portrait;
}

/** 当前位置（1 起）：给角标 `1/3` 用；不在列表里返回 0 */
export function variantIndexOf(ps: VariantPortrait[], currentId: number | null): number {
  return variantSlots(ps).findIndex((s) => s.portrait.id === currentId) + 1;
}

/**
 * 当前这张的表情名（给角标用）。
 *
 * **不再退回版本号**（2026-10-25 用户："详图里还有 v1"）：素材只有一个版本时 `v1` 是纯噪音，
 * 没打表情标签就不显示名字，只留 `1/3` 这种位置指示。
 */
export function labelOfVariant(p: VariantPortrait | null): string {
  if (!p) return "";
  return expressionTagOf(p);
}
