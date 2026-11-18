/// 关键记忆正文的显示清洗（v3.5.1）+ 按关系组分组（2026-11-18）。
///
/// ## 一、正文清洗
///
/// 记忆文件是 Markdown，正文形如
///
/// ```text
/// # 第 4 天 · 表白
///
/// [夏婉琴]“我爱你，跟我在一起吧。”（她把这句话在心里练了三年）
///
/// > 由系统从剧情里提炼（命中「我爱你」）。这是这个分支里第一次发生，后续同类事件不再单独记录。
/// ```
///
/// 但卡片上方**已经显示**了「表白 · 第 4 天」—— 再把标题渲染一遍就是重复；
/// 而且按纯文本渲染会把 `#` 和 `>` 直接露出来。所以这里把正文拆成
/// 「描述」+「系统备注」两部分，界面分别用正常字号和小字显示。
///
/// ## 二、按关系组分组
///
/// v3.16 起一张卡带**多参与者**（`characters: a,b`，老卡的单值 `character:` 也读）。
/// 「a 向 b 表白」这张卡两边都该记得，所以界面按**参与者集合**分组：a×b 与 b×a 是同一组。
/// 这套口径与后端 `memory::group_by_participants` 一致（集合相等，顺序无关），
/// 这里只是给界面用的一份镜像 —— 后端的注入逻辑才是权威，别在界面上另立一套判断。

export type MemoryBodyView = {
  /// 正文（去掉标题与引用块），可能为空
  text: string;
  /// 系统备注（原来的 `> …` 行），可能为空
  note: string;
};

/// 记忆卡的两个「人」：AI 侧（写卡时 AI 演的）与用户侧（作者扮演的）。
/// 能对上号才给 —— 对不上（老卡单值 / 自由模式 / 全局卡）就保持 `[用户]/[AI]` 原样。
export type MemorySides = { ai: string; user: string };

/// 按 `characters` / `ai_side` 解出两侧名字（与后端 `memory::ai_name_of` 同口径）。
///
/// 优先显式 `ai_side`；老卡没有这一行时退回隐含约定 —— 只有**恰好两个**参与者
/// 才敢假定 `characters[0]` 是 AI 侧（写卡顺序 `[AI, 用户]`）。只有一个时不猜。
export function sidesOf(m: MemoryDto): MemorySides | null {
  const chars = (m.characters ?? []).map((c) => (c ?? "").trim()).filter(Boolean);
  const explicit = (m.ai_side ?? "").trim();
  let ai = "";
  let user = "";
  if (explicit) {
    ai = explicit;
    user = chars.find((c) => c !== explicit) ?? "";
  } else if (chars.length === 2) {
    ai = chars[0];
    user = chars[1];
  }
  if (!ai || !user) return null;
  return { ai, user };
}

/// 行首 `[用户]` / `[AI]` → 人名；写死的「她的话：」→「{名字}的话：」。
/// 只动标签，不动句子里的「我」「你」（台词是原话，改了就不是原话了）。
function renameLabels(text: string, sides: MemorySides | null): string {
  if (!sides) return text;
  const out = text
    .split("\n")
    .map((line) => {
      const m = line.match(/^(\s*)\[用户\]/);
      if (m) return `${m[1]}[${sides.user}]${line.slice(m[0].length)}`;
      const a = line.match(/^(\s*)\[AI\]/);
      if (a) return `${a[1]}[${sides.ai}]${line.slice(a[0].length)}`;
      return line;
    })
    .join("\n");
  // 老卡的摘要行兜底：「第 N 次表白：…」→「顾宸熙第 N 次向夏婉琴表白：…」
  // 新卡写的时候已经是带名字的句子，这里匹配不上，是 no-op。
  return out
    .split("\n")
    .map((line) => line.replace(/^第 (\d+) 次表白：/, `${sides.user}第 $1 次向${sides.ai}表白：`))
    .join("\n")
    .split("她的话：")
    .join(`${sides.ai}的话：`);
}

export function memoryBodyView(body: string, sides?: MemorySides | null): MemoryBodyView {
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
  return { text: renameLabels(textLines.join("\n"), sides ?? null), note: noteLines.join("\n") };
}

/// 一条记忆（`memory_list` 命令的返回形状）。
///
/// `characters` 是 v3.16 的**参与者名单**（`characters: a,b`）；空数组 = 全局
/// （没有归属行的卡：老版本写的 / 自由模式 / 旁观事件）—— 任何角色视角都看得见，
/// 界面上归到「全局」组。
export type MemoryDto = {
  id: string;
  kind: string;
  label: string;
  day_label: string;
  branch: string | null;
  importance: number;
  body: string;
  promise_state?: string;
  created_at: string;
  characters: string[];
  /// 写卡时 AI 演的是谁（v3.18）：正文 `[AI]` = 这个人、`[用户]` = 对面那个。
  /// 老卡没有这一行 → `sidesOf` 退回 `characters` 顺序推断（仅限恰好两人）。
  ai_side?: string;
};

/// 一个关系组：`key` 是排序去重后的参与者名单（空数组 = 全局组）。
export type MemoryGroup = {
  key: string[];
  /// 界面上的组名：`夏婉琴 × 顾宸熙` / `全局（无归属）`
  label: string;
  items: MemoryDto[];
};

/// 全局组的显示名（唯一的出处，界面别再手写）
export const GLOBAL_GROUP_LABEL = "全局（无归属）";

/// 一张卡的参与者集合：去空白、去重、排序（排序是为了 a×b 与 b×a 落到同一组）。
function keyOf(m: MemoryDto): string[] {
  const seen = new Set<string>();
  for (const c of m.characters ?? []) {
    const s = (c ?? "").trim();
    if (s) seen.add(s);
  }
  return [...seen].sort();
}

/// 按参与者集合分组。
///
/// 顺序与后端同口径：**组内卡片多的在前**，一样多按组名排（保证界面每次一样，
/// 不会因为 Map 顺序或后端返回顺序变化而跳来跳去）。
export function groupMemories(items: MemoryDto[]): MemoryGroup[] {
  const groups: MemoryGroup[] = [];
  for (const m of items) {
    const key = keyOf(m);
    const hit = groups.find((g) => sameKey(g.key, key));
    if (hit) hit.items.push(m);
    else groups.push({ key, label: key.length ? key.join(" × ") : GLOBAL_GROUP_LABEL, items: [m] });
  }
  groups.sort((a, b) => b.items.length - a.items.length || cmpKey(a.key, b.key));
  return groups;
}

/// 按角色切成三摞（角色详情卡里的弹窗就按这个排版）：
/// - `mine`：名单里**有她**的组（a×b、a×c 都算她的）；
/// - `global`：没有归属的组（0 或 1 个）；
/// - `others`：跟她无关的组 —— 界面上折叠，可展开。
///
/// 为什么全局组要单独拿出来：删掉时间线底部那个总面板之后，无归属的卡不属于
/// 任何角色，只在「全局」这一摞里露脸；不显示它，这些卡就彻底没有入口了。
export function splitGroups(
  groups: MemoryGroup[],
  character: string,
): { mine: MemoryGroup[]; global: MemoryGroup[]; others: MemoryGroup[] } {
  const name = (character ?? "").trim();
  const mine: MemoryGroup[] = [];
  const global: MemoryGroup[] = [];
  const others: MemoryGroup[] = [];
  for (const g of groups) {
    if (g.key.length === 0) global.push(g);
    else if (name && g.key.includes(name)) mine.push(g);
    else others.push(g);
  }
  return { mine, global, others };
}

/// 这个角色名下有几张卡（按钮上的数字）。
///
/// 只数「含她的」，不含全局卡 —— 按钮写的是"她的记忆有几条"，把全局算进去
/// 会让每个角色的数字都虚高同一截。
export function countFor(items: MemoryDto[], character: string): number {
  const name = (character ?? "").trim();
  if (!name) return 0;
  return items.filter((m) => keyOf(m).includes(name)).length;
}

function sameKey(a: string[], b: string[]): boolean {
  return a.length === b.length && a.every((x, i) => x === b[i]);
}

/// 按字符序比较组名（与后端 `join(",")` 的字典序对齐，别用 localeCompare：
/// 不同运行环境的排序规则不一样，界面顺序会漂）
function cmpKey(a: string[], b: string[]): number {
  const x = a.join(",");
  const y = b.join(",");
  return x === y ? 0 : x < y ? -1 : 1;
}
