// 把提示词切成「段」并分成**通用**与**有差异**两组（2026-11-14）。
//
// 双池对比要看的是"两个视角哪里不一样"，而 system 里大半是两边共用的规则
// （基础指令 / 输出排版 / 数据字典 / 台词口径 / 时间基准 / 前史 / 世界观 / 配合度 / 关系态势 / 情绪表演）。
// 那些只该看一遍；真正要比的是角色卡、关系、【你对面的他】。
//
// 切分规则：`【标题】` 开头算新段；第一个 `【` 之前的正文算「基础指令」段。
// 纯函数，好测。

export type Seg = { title: string; body: string };

/** 按 `【…】` 切段；首个标题前的正文归到「基础指令」 */
export function splitSegments(text: string): Seg[] {
  const out: Seg[] = [];
  const lines = text.split("\n");
  let title = "基础指令";
  let buf: string[] = [];
  const flush = () => {
    const body = buf.join("\n").trim();
    if (body) out.push({ title, body });
    buf = [];
  };
  for (const line of lines) {
    const m = line.match(/^【([^】]+)】/);
    if (m) {
      flush();
      title = `【${m[1]}】`;
      // 标题行后面的剩余内容也算这一段
      const rest = line.slice(m[0].length).trim();
      if (rest) buf.push(rest);
    } else {
      buf.push(line);
    }
  }
  flush();
  return out;
}

export type SegmentDiff = {
  /** 两边一模一样、只显示一次的段 */
  common: Seg[];
  /** 有差异（或只在一侧出现）的段，按标题配对；一侧缺失时为 null */
  differing: { title: string; global: Seg | null; role: Seg | null }[];
};

/** 逐段比较：同标题同内容 → common；其余 → differing（同标题不同内容会配对到同一行） */
export function compareSegments(globalText: string, roleText: string): SegmentDiff {
  const g = splitSegments(globalText);
  const r = splitSegments(roleText);
  const byTitle = new Map<string, { g?: Seg; r?: Seg }>();
  for (const s of g) byTitle.set(s.title, { ...(byTitle.get(s.title) ?? {}), g: s });
  for (const s of r) byTitle.set(s.title, { ...(byTitle.get(s.title) ?? {}), r: s });
  const common: Seg[] = [];
  const differing: SegmentDiff["differing"] = [];
  for (const [title, pair] of byTitle) {
    if (pair.g && pair.r && pair.g.body === pair.r.body) common.push(pair.g);
    else differing.push({ title, global: pair.g ?? null, role: pair.r ?? null });
  }
  return { common, differing };
}
