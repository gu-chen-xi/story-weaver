// 纯函数：解析 / 格式化 / 预算计算。
// 这些函数**不碰 React、不碰 invoke**，因此可以单独测、也可以被其它模块复用。

import { DIRECTOR_TAG_RE, SECTION_COLORS, EVENT_LABEL, REL_STATES } from "../constants";
import type { ScriptBlock, DebugMessage, PromptSection, DebugLike, BudgetComputed } from "../types";

export function parseScriptBlocks(content: string, charNames: string[]): ScriptBlock[] {
  const paras = content.split(/\n+/).map((s) => s.trim()).filter(Boolean);
  const blocks: ScriptBlock[] = [];
  let cur = "";
  let lastChar = "";
  // 从引号后的描述推断说话人：医生说/她开口/母亲…
  const resolveAfter = (after: string): string => {
    const m = after.match(/^[，,、\s]*([^，。！？\s]{1,8}?)(?:说|道|开口|低语|喊|吼|又问|又说|接着说|嘟囔|着|道了句)/);
    if (m && m[1]) {
      const n = m[1].replace(/(说|道|问|喊|吼|开口)$/, "");
      if (n === "她" || n === "他") return lastChar || cur || "AI";
      if (n.length >= 1) return n;
    }
    if (/^[，,、\s]*[她他]/.test(after)) return lastChar || cur || "AI";
    const short = after.match(/^[，,、\s]*([^，。！？\s]{1,4})/);
    if (short && short[1]) {
      const hit = charNames.find((n) => n === short[1]);
      if (hit) return hit;
    }
    return cur || "AI";
  };
  for (let i = 0; i < paras.length; i++) {
    const p = paras[i];
    const next = paras[i + 1];
    // 0) 强标记台词：[角色名]“台词”（AI 必须遵守的格式）
    const bm = p.match(/^\s*\[([^\[\]]{1,12})\]\s*(.+)$/);
    if (bm) {
      const nm = bm[1].trim();
      if (nm && !/[。！？!?]/.test(nm)) {
        blocks.push({ type: "dialog", name: nm, text: bm[2].trim() });
        lastChar = nm;
        continue;
      }
    }
    // 1) 角色名行：精确匹配角色库；或短行（≤6字、无句末标点、非括号/引号、不含常见叙述词）且后跟内容
    const isExact = charNames.some((n) => n && n === p);
    const isLikely =
      p.length <= 6 &&
      !/[。！？!?：:]$/.test(p) &&
      !/^[（(]/.test(p) &&
      !/^[「“"]/.test(p) &&
      !/(的|了|在|很|着|过|说|道|想|像|是|有|她|他|个|块|点|声|次|天|年)/.test(p) &&
      !!next;
    if (isExact || isLikely) {
      cur = p;
      blocks.push({ type: "speaker", name: "", text: p });
      continue;
    }
    // 2) 台词行：以“名字：/名字说：”开头 → 人物对话
    const m = p.match(/^([^：:]{1,14})[：:]\s*(.+)$/);
    if (m) {
      const rawName = m[1].trim();
      const name = rawName.replace(/(说|道|问|喊|吼)$/, "");
      blocks.push({ type: "dialog", name, text: m[2].trim() });
      if (name !== "AI") lastChar = name;
      continue;
    }
    // 3) 台词行：以引号开头 → 从引号后描述推断说话人
    const qm = p.match(/^[「“"]([\s\S]*?)[」”"]/);
    if (qm) {
      const after = p.slice(qm[0].length);
      let name = "";
      if (after.trim()) name = resolveAfter(after);
      if (!name) name = cur || "AI";
      blocks.push({ type: "dialog", name, text: p });
      if (name !== "AI") lastChar = name;
      continue;
    }
    // 4) 动作行：整行（圆括号）→ 动作/神态
    if (/^[（(].+[）)]$/.test(p)) {
      blocks.push({ type: "action", name: "", text: p });
      continue;
    }
    // 4.5) 沉浸式心声：整行 *……* → 角色内心原声（渲染为心声样式，不显示星号）
    const im = p.match(/^\*+\s*([\s\S]+?)\s*\*+$/);
    if (im && im[1].trim()) {
      blocks.push({ type: "inner", name: "", text: im[1].trim() });
      continue;
    }
    // 5) 其余：旁白，更新最近角色
    blocks.push({ type: "narr", name: "", text: p });
    let last = "";
    for (const n of charNames) if (p.includes(n)) last = n;
    if (last) lastChar = last;
  }
  return blocks;
}

// ===== 生成参数页辅助（Token 预算 / Prompt 预览）=====

/**
 * 估算 token 数：按字符类别加权（汉字/全角 1、英文字母数字 0.25、空白 0.15、ASCII 标点 0.5）。
 *
 * 2026-10-28 换掉「字数 × 1.5」：英文提示词上线后旧口径会把英文虚报 3 倍、中文高估 ~50%。
 * 系数与后端 `src-tauri/src/tokens.rs` 保持一致，改一边必须同步另一边。
 */
export const estimateTokens = (text: string): number => {
  if (!text) return 0;
  let acc = 0;
  for (const ch of text) {
    const code = ch.codePointAt(0) ?? 0;
    if (code < 128) {
      if (/[A-Za-z0-9]/.test(ch)) acc += 0.25;
      else if (/\s/.test(ch)) acc += 0.15;
      else acc += 0.5;
    } else {
      acc += 1;
    }
  }
  return Math.max(1, Math.ceil(acc));
};

export function parseDebugHistory(json: string): DebugMessage[] {
  try {
    const arr: unknown = JSON.parse(json);
    if (!Array.isArray(arr)) return [];
    return arr.map((m) => {
      if (Array.isArray(m)) return { role: String(m[0] ?? ""), content: String(m[1] ?? "") };
      const o = (m ?? {}) as { role?: unknown; content?: unknown };
      return { role: String(o.role ?? ""), content: String(o.content ?? "") };
    });
  } catch {
    return [];
  }
}

export function splitPromptSections(text: string): PromptSection[] {
  if (!text) return [];
  const re = /【([^】]{1,20})】/g;
  const hits: { name: string; idx: number }[] = [];
  let m = re.exec(text);
  while (m) {
    hits.push({ name: m[1], idx: m.index });
    m = re.exec(text);
  }
  if (hits.length === 0) return [{ name: "系统提示词", text }];
  const out: PromptSection[] = [];
  const head = text.slice(0, hits[0].idx).trim();
  if (head) out.push({ name: "基础指令", text: head });
  hits.forEach((h, i) => {
    const end = i + 1 < hits.length ? hits[i + 1].idx : text.length;
    out.push({ name: h.name, text: text.slice(h.idx, end).trim() });
  });
  return out;
}

export const sectionColor = (name: string) => {
  const hit = Object.keys(SECTION_COLORS).find((k) => name.startsWith(k));
  return hit ? SECTION_COLORS[hit] : "#5b8cff";
};

// 关系状态：固定三选一（含义会随关系段注入提示词）

export const normRelState = (s: string) => (REL_STATES as readonly string[]).includes(s) ? s : "稳定";

// ===== 导演指令（沉浸对话专用）=====
// 格式：〔导演指令〕<指令内容>（可多行）\n\n<用户正文>；后端按同一前缀剥离，改这里要同步改 relations.rs 的 DIRECTOR_TAG

export function parseDirector(text: string): string {
  const t = (text || "").replace(/^\s+/, "");
  if (!DIRECTOR_TAG_RE.test(t)) return "";
  return t.replace(DIRECTOR_TAG_RE, "").split(/\n{2,}/)[0].trim();
}
/** 去掉导演指令行，只留用户正文（气泡正文 / 剧本渲染用） */

export function stripDirector(text: string): string {
  const t = (text || "").replace(/^\s+/, "");
  if (!DIRECTOR_TAG_RE.test(t)) return text || "";
  const rest = t.replace(DIRECTOR_TAG_RE, "").split(/\n{2,}/);
  return rest.length > 1 ? rest.slice(1).join("\n\n").trim() : "";
}

// 时间线节点（后端 timeline::TimelineNode 的镜像；day_summary 为空表示这一天还没生成日摘要）
// 第五批 ③：故事分支（与后端 branch::Branch 对应）

export const eventLabel = (k: string) => EVENT_LABEL[k] || "其它事件";



// 评估维度中文名（后端 key → 界面）

export const fmtNum = (n: number) => n.toLocaleString();


// ===== 导演代价（沉浸对话专用，2026-10-27）=====
// 格式：〔导演代价〕类型：理由（AI 回复的**最后一行**）；后端 director.rs 解析、算分、记账。
// 这一行只给软件对账用 —— 气泡 / 编辑框 / 导出都不该出现它（和〔态度:…〕同一个待遇）。

/** 协议标记；改这里要同步改 `src-tauri/src/director.rs` 的 COST_TAG */
export const COST_TAG = "〔导演代价〕";

/**
 * 剥掉回复**最后一行**的导演代价协议行（只认最后一行，和后端 strip_receipt 同规则）。
 *
 * 为什么不 filter 全部行：正文中间出现同名字样是剧情文本（"她在账本上写〔导演代价〕…"），
 * 删掉就是改用户的剧情。只有末尾那一行属于协议。
 */
/** 文本最后一个非空行（赦免盒子里显示"她的最后一句"） */
export function lastLineOf(text: string): string {
  const lines = (text || "").split("\n").map((l) => l.trim()).filter(Boolean);
  return lines.length ? lines[lines.length - 1] : "";
}

export function stripCostMarker(content: string): string {
  const text = content || "";
  const lines = text.split("\n");
  const start = protocolBlockStart(lines);
  if (start >= lines.length) return text;
  const kept = lines.filter((l, i) => !(i >= start && l.trim().startsWith(COST_TAG)));
  const rest = kept.join("\n").trim();
  // 剥完变空（模型只写了协议行）→ 退回原文：宁可多一行，也不要存一条空气泡
  return rest || text;
}

/**
 * 末尾「协议区」的起始行号：从最后一行往上跳过空行、导演代价行、态度标记行，
 * 遇到正文就停。与后端 `memory::protocol_block_start` 同一口径。
 *
 * 为什么要"协议区"而不是"最后一行"：同一轮里两条协议都要写（态度 + 代价），
 * 谁在前谁在后都可能 —— 2026-10-27 实测里收据被态度标记挤到倒数第二行，只认最后一行就会漏账。
 */
function protocolBlockStart(lines: string[]): number {
  let start = lines.length;
  for (let i = lines.length - 1; i >= 0; i--) {
    const t = lines[i].trim();
    if (t === "" || t.startsWith(COST_TAG) || isStanceMarkerLine(t)) {
      start = i;
      continue;
    }
    break;
  }
  return start;
}

export function computeBudget(dbg: DebugLike | null, ctxWindow: number): BudgetComputed {
  // system 段按【】切分；尾部「上下文更新」块单独成节（它不在 system_prompt 里）
  const sysSections = dbg ? splitPromptSections(dbg.system_prompt) : [];
  const tailText = (dbg?.tail_block || "").trim();
  const tailSection: PromptSection | null = tailText ? { name: "上下文更新（尾部）", text: tailText } : null;
  const history = dbg ? parseDebugHistory(dbg.history_json) : [];
  const lastUser = history.length && history[history.length - 1].role === "user" ? history[history.length - 1] : null;
  const priorHistory = lastUser ? history.slice(0, -1) : history;
  const sysTokens = sysSections.reduce((s, x) => s + estimateTokens(x.text), 0) + (tailSection ? estimateTokens(tailSection.text) : 0);
  const histTokens = priorHistory.reduce((s, x) => s + estimateTokens(x.content), 0);
  const userTokens = lastUser ? estimateTokens(lastUser.content) : (dbg ? estimateTokens(dbg.user_text) : 0);
  // 用量：优先用快照里后端算好的值；**老快照该字段为 0**（那时还没记这个字段）时回退到自算值。
  // 注意 0 不是 nullish —— 早期写成 `?? 自算值` 导致老快照显示"已用 0"（用户 2026-10-18 报）。
  // 回退时 usedRecalculated = true，界面上会标注"老快照按当前口径重算"，避免误当成历史真实值。
  const snapUsed = dbg?.estimated_tokens ?? 0;
  const usedRecalculated = dbg != null && snapUsed <= 0;
  const usedTokens = snapUsed > 0 ? snapUsed : (sysTokens + histTokens + userTokens);
  // 输入预算优先用快照里后端算好的口径（A 子项：后端算一次，前端不再自己硬编码）；
  // 老快照没有该字段（=0）时，回退成窗口 × 0.8 − 输出预留(4000+800)
  const snapBudget = dbg?.input_budget ?? 0;
  const inputBudget = snapBudget > 0 ? snapBudget : Math.max(Math.round(ctxWindow * 0.8) - 4800, 1);
  const remain = Math.max(inputBudget - usedTokens, 0);
  const pctUsed = Math.min(100, Math.round((usedTokens / inputBudget) * 100));
  return { sysSections, tailSection, history, lastUser, priorHistory, sysTokens, histTokens, userTokens, usedTokens, usedRecalculated, inputBudget, remain, pctUsed };
}

/**
 * 剥掉 AI 回复末尾的「表态标记」（`〔态度:接受〕` / `〔态度:拒绝〕`）。
 *
 * 标记是**协议**：只在回复最后一行、只给软件判定用（记忆卡用它定"接受/拒绝"），
 * 用户不该看见它，编辑框里也不该带它 —— 否则用户一改就把它删了，判定就退化成猜。
 *
 * 和后端 `memory::strip_marker` 同一套规则：整行匹配、行长 ≤ 24。
 */
export function stripStanceMarker(content: string): string {
  return content
    .split("\n")
    .filter((line) => !isStanceMarkerLine(line))
    .join("\n")
    .trim();
}

/**
 * 这一整行是不是表态标记 —— **只判形状，不判值**（容忍全角/半角冒号与首尾标点）。
 *
 * 2026-10-27 实测：模型自创过「〔态度 冷淡〕」这种表外值。解析可以不认（退回台词打分），
 * 但**展示必须剥掉** —— 否则气泡里会冒出一个小方块，用户不知道那是什么。
 */
export function isStanceMarkerLine(line: string): boolean {
  const t = line.trim().replace(/^[。.,，！!\s]+|[。.,，！!\s]+$/g, "");
  return t.length <= 24 && t.startsWith("〔态度") && t.endsWith("〕");
}