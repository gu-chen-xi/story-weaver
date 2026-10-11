import { useEffect, useRef, useState } from "react";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

type Story = {
  id: number;
  title: string;
  time_origin: string;
  created_at: string;
  updated_at: string;
};

type Character = {
  id: number;
  name: string;
  alias: string;
  role: string;
  faction: string;
  age: string;
  gender: string;
  first_appearance: string;
  one_liner: string;
  profile: string;
  speech_style: string;
  traits: string;
  current_status: string;
  stat_stamina: number;
  stat_sanity: number;
  stat_emotion: number;
  stat_screen: number;
  version: number;
};

type CharacterUsage = {
  id: number;
  dialogue_count: number;
  node_count: number;
};

type CharacterForm = {
  name: string;
  alias: string;
  role: string;
  faction: string;
  age: string;
  gender: string;
  first_appearance: string;
  one_liner: string;
  profile: string;
  speech_style: string;
  traits: string;
  stat_stamina: number;
  stat_sanity: number;
  stat_emotion: number;
  stat_screen: number;
};

const EMPTY_CHAR_FORM: CharacterForm = {
  name: "", alias: "", role: "未定位", faction: "", age: "", gender: "",
  first_appearance: "", one_liner: "", profile: "", speech_style: "", traits: "",
  stat_stamina: 50, stat_sanity: 50, stat_emotion: 50, stat_screen: 50,
};

type WorldSetting = {
  id: number;
  key: string;
  value: string;
  is_hard_rule: number;
  version: number;
  category: string;
  meta_json: string;
};

type WorldConflict = {
  title: string;
  level: string;
  left_id: number;
  right_id: number;
  left_label: string;
  left_text: string;
  left_source: string;
  right_label: string;
  right_text: string;
  right_source: string;
};

type Relation = {
  id: number;
  from_character_id: number;
  to_character_id: number;
  relation_type: string;
  direction: string;
  strength: number;
  state: string;
  description: string;
  evidence: string;
  version: number;
  split_records: { text: string; role: string }[];
  split_matched: boolean;
};

// ===== 深色主题（参考 STORY DECK 原型 v1）=====
const T = {
  win: "#16181d",
  panel: "#1c1f26",
  panel2: "#22262f",
  panel3: "#2a2f3a",
  line: "#2e333d",
  lineSoft: "#262a33",
  text1: "#e8eaf0",
  text2: "#a3a9b8",
  text3: "#6f7686",
  accent: "#FF7A18",
  accentSoft: "rgba(255,122,24,.14)",
  aiBubble: "#23272f",
  ok: "#37c978",
  warn: "#f5b544",
  danger: "#f0554d",
};

// 敏感词过滤默认列表（后续可在设置中自定义）
const SENSITIVE_WORDS = ["傻逼", "草泥马", "去你妈", "操你妈", "狗日", "他妈的"];

// 插件位（插件系统后置，仅展示规划）
const PLUGIN_PLACEHOLDERS = [
  { icon: "🦙", name: "Ollama 本地模型", desc: "接入本地大模型，实现纯离线剧情生成（依赖插件系统，后置）", status: "规划中" },
  { icon: "🧠", name: "评估判断模型", desc: "独立的剧情 / 设定评估插件，用于后台演进分析（后置）", status: "规划中" },
  { icon: "🎨", name: "立绘生成（云端 / 本地双通道）", desc: "角色立绘生成与本地自由生成切换（后置）", status: "规划中" },
];

// ===== 立绘素材（后端 portraits 表，library\default\shared\portraits） =====
type PortraitItem = { id: number; name: string; role: string; ver: string; ratio: string; width: number; height: number; size: string; size_bytes: number; date: string; pinned: boolean; tags: string[]; g: string; path?: string | null; story_id?: number | null; character_id?: number | null; bound_story?: string | null; bound_character?: string | null; group: string; prompt: string; negative: string; seed: string; steps: string; cfg: string; model: string; versions: { version: number; desc: string; date: string }[] };
const PORTRAIT_GRADS: Record<string, string> = {
  g1: "linear-gradient(160deg,#4a5f8a,#22293a)",
  g2: "linear-gradient(160deg,#7b5cff,#3a2b6b)",
  g3: "linear-gradient(160deg,#c9584f,#5a2521)",
  g4: "linear-gradient(160deg,#3f8f6d,#1e3d30)",
  g5: "linear-gradient(160deg,#d0913c,#5a3d17)",
  g6: "linear-gradient(160deg,#4f8fb8,#22404f)",
};

// ===== 剧本段落解析：旁白 / 人物对话 =====
type ScriptBlock = { type: "narr" | "dialog" | "action" | "speaker" | "inner"; name: string; text: string };
function parseScriptBlocks(content: string, charNames: string[]): ScriptBlock[] {
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
type PromptSection = { name: string; text: string };
type DebugMessage = { role: string; content: string };

const estimateTokens = (text: string) => (text ? Math.ceil(Array.from(text).length * 1.5) : 0);

function parseDebugHistory(json: string): DebugMessage[] {
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

function splitPromptSections(text: string): PromptSection[] {
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

const SECTION_COLORS: Record<string, string> = {
  "基础指令": "#5b8cff",
  "角色": "#8f6bff",
  "关系": "#b79bff",
  "世界观": "#37c978",
  "当前模式": "#f5b544",
  "本次聚焦": "#f5b544",
  "剧情约束": "#f0554d",
  "篇幅要求": "#5b8cff",
  "时空提示": "#4f8fb8",
};

const sectionColor = (name: string) => {
  const hit = Object.keys(SECTION_COLORS).find((k) => name.startsWith(k));
  return hit ? SECTION_COLORS[hit] : "#5b8cff";
};

// 关系状态：固定三选一（含义会随关系段注入提示词）
const REL_STATES = ["稳定", "紧张", "破裂"] as const;
const REL_STATE_HINT: Record<string, string> = {
  "稳定": "冲突止于拌嘴",
  "紧张": "容易升级为争执",
  "破裂": "出现回避、断绝、伤害性言行",
};
/** 老数据兜底：不在三个固定值里的状态统一按「稳定」处理 */
const normRelState = (s: string) => (REL_STATES as readonly string[]).includes(s) ? s : "稳定";

// ===== 导演指令（沉浸对话专用）=====
// 格式：〔导演指令〕<指令内容>（可多行）\n\n<用户正文>；后端按同一前缀剥离，改这里要同步改 relations.rs 的 DIRECTOR_TAG
const DIRECTOR_TAG = "〔导演指令〕";
const DIRECTOR_TAG_RE = /^\s*〔导演指令〕/;
/** 取出一条用户消息里的导演指令；没有则返回空串 */
function parseDirector(text: string): string {
  const t = (text || "").replace(/^\s+/, "");
  if (!DIRECTOR_TAG_RE.test(t)) return "";
  return t.replace(DIRECTOR_TAG_RE, "").split(/\n{2,}/)[0].trim();
}
/** 去掉导演指令行，只留用户正文（气泡正文 / 剧本渲染用） */
function stripDirector(text: string): string {
  const t = (text || "").replace(/^\s+/, "");
  if (!DIRECTOR_TAG_RE.test(t)) return text || "";
  const rest = t.replace(DIRECTOR_TAG_RE, "").split(/\n{2,}/);
  return rest.length > 1 ? rest.slice(1).join("\n\n").trim() : "";
}

// 时间线节点（后端 timeline::TimelineNode 的镜像；day_summary 为空表示这一天还没生成日摘要）
type TimelineNodeDto = {
  id: number; date_label: string; sort_order: number; node_type: string;
  is_flashback: number; tags: string; day_summary?: string;
};
// 千分位格式化（模块级，供设置页与生成参数页共用；不写死语言，按系统区域自动分隔）
const fmtNum = (n: number) => n.toLocaleString();

type DebugLike = {
  system_prompt: string; tail_block?: string | null; history_json: string; user_text: string; estimated_tokens: number;
  // 预算口径（老快照缺省为 0，computeBudget 会回退到配置值）
  context_window?: number; reserve_ratio?: number; max_tokens?: number; safety_margin?: number; input_budget?: number;
  // 跨天历史
  day_window?: number; day_n?: string; n_value?: number; avg_msg_tokens?: number;
};

type BudgetComputed = {
  sysSections: PromptSection[];
  tailSection: PromptSection | null;   // 尾部「上下文更新」块（易变，放在历史之后）
  history: DebugMessage[];
  lastUser: DebugMessage | null;
  priorHistory: DebugMessage[];
  sysTokens: number;
  histTokens: number;
  userTokens: number;
  usedTokens: number;
  inputBudget: number;
  remain: number;
  pctUsed: number;
};

function computeBudget(dbg: DebugLike | null, ctxWindow: number): BudgetComputed {
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
  const usedTokens = dbg?.estimated_tokens ?? (sysTokens + histTokens + userTokens);
  // 输入预算优先用快照里后端算好的口径（A 子项：后端算一次，前端不再自己硬编码）；
  // 老快照没有该字段（=0）时，回退成窗口 × 0.8 − 输出预留(4000+800)
  const snapBudget = dbg?.input_budget ?? 0;
  const inputBudget = snapBudget > 0 ? snapBudget : Math.max(Math.round(ctxWindow * 0.8) - 4800, 1);
  const remain = Math.max(inputBudget - usedTokens, 0);
  const pctUsed = Math.min(100, Math.round((usedTokens / inputBudget) * 100));
  return { sysSections, tailSection, history, lastUser, priorHistory, sysTokens, histTokens, userTokens, usedTokens, inputBudget, remain, pctUsed };
}

function App() {
  // ===== 故事库（M1）=====
  const [stories, setStories] = useState<Story[]>([]);
  const [currentStory, setCurrentStory] = useState<Story | null>(null);
  const [showNew, setShowNew] = useState(false);
  const [newTitle, setNewTitle] = useState("");
  const [newOrigin, setNewOrigin] = useState("2026-01-01");
  const [msg, setMsg] = useState("");

  // ===== 设置（M0/M2）=====
  const [showSettings, setShowSettings] = useState(false);
  const [baseUrl, setBaseUrl] = useState("");
  const [modelName, setModelName] = useState("");
  const [apiKeyInput, setApiKeyInput] = useState("");
  const [testBusy, setTestBusy] = useState(false);
  const [testResult, setTestResult] = useState<{ ok: boolean; detail: string; reply?: string } | null>(null);

  // ===== 请求快照（调试）=====
  type DebugReq = {
    id: number; node_id: number | null; model: string | null;
    system_prompt: string;
    tail_block: string | null;   // 尾部「上下文更新」块（当前模式 / 篇幅 / 数值与状态 / 聚焦 / 约束 / 时空 / 导演指令）；老快照为空
    // 本次实际预算口径（A 子项）：老快照为 0，前端回退到配置值计算
    context_window: number; reserve_ratio: number; max_tokens: number; safety_margin: number; input_budget: number;
    // 跨天历史（B 子项）：保留几天的原文 / 每天各取几条上限（逗号分隔）/ 由预算倒推出来的 N
    day_window: number; day_n: string; n_value: number; avg_msg_tokens: number;
    history_json: string; user_text: string;
    ai_reply: string | null; estimated_tokens: number; n_used: number;
    status: string; error: string | null; created_at: string;
  };
  const [debugList, setDebugList] = useState<DebugReq[]>([]);
  const [selectedDebug, setSelectedDebug] = useState<DebugReq | null>(null);

  // ===== 角色库（工作区内）=====
  const [chars, setChars] = useState<Character[]>([]);
  // ===== 角色管理页（A 卡墙 / D 关系图谱 / G 新建表单）=====
  const [charTab, setCharTab] = useState<"cards" | "relations">("cards");
  const [charsLoading, setCharsLoading] = useState(false);
  const [charSearch, setCharSearch] = useState("");
  const [charFilter, setCharFilter] = useState("全部");
  const [charGraphRootId, setCharGraphRootId] = useState<number | null>(null);
  const [charUsage, setCharUsage] = useState<CharacterUsage[]>([]);
  const [charFormOpen, setCharFormOpen] = useState(false);
  const [charEditingId, setCharEditingId] = useState<number | null>(null);
  const [charError, setCharError] = useState("");
  const [charForm, setCharForm] = useState<CharacterForm>({ ...EMPTY_CHAR_FORM });

  // ===== 世界观（工作区内）=====
  const [view, setView] = useState<"chars" | "world" | "relations" | "story" | "debug" | "proposals" | "cache" | "timeline" | "stories" | "plugins" | "portrait" | "params" | "home">("home");
  const [navMode, setNavMode] = useState<"旁白" | "对话">("对话"); // 旁白/对话两态
  const [myRole, setMyRole] = useState("自由"); // 对话模式：我扮演的角色
  const [aiRole, setAiRole] = useState("自由"); // 对话模式：AI 扮演的角色
  const [showAt, setShowAt] = useState(false);   // @角色 菜单
  const [showTag, setShowTag] = useState(false); // #标签 菜单
  const [tagCustom, setTagCustom] = useState("");
  const PRESET_TAGS = ["主线", "日常", "回忆", "倒叙", "插曲", "战斗", "情感"];
  const [sensitiveFilter, setSensitiveFilter] = useState(false);   // 敏感词过滤开关（拦截输入 + 过滤AI输出）
  const [constraints, setConstraints] = useState<string[]>(() => { try { return JSON.parse(localStorage.getItem("ai.constraints") || "[]"); } catch { return []; } });   // 剧情约束（注入提示词，持久化）
  const [lenLevel, setLenLevel] = useState<"简单" | "详细">("简单");   // 篇幅：简单描写 / 详细展开
  const [memoryCtx, setMemoryCtx] = useState(true);                     // 记忆上下文：是否携带历史
  const [showConstraintInput, setShowConstraintInput] = useState(false);
  const [constraintDraft, setConstraintDraft] = useState("");
  // 导演指令（沉浸对话专用，一次性：附加到下一次发送后自动清空）
  const [directors, setDirectors] = useState<string[]>(() => { try { return JSON.parse(localStorage.getItem("ai.directors") || "[]"); } catch { return []; } });
  const [showDirectorInput, setShowDirectorInput] = useState(false);
  const [directorDraft, setDirectorDraft] = useState("");
  const [timelineView, setTimelineView] = useState<"list" | "cal">("list");   // 时间线视图：列表/日历
  const [calOffset, setCalOffset] = useState(0);                               // 日历翻页偏移（月）
  const [showExport, setShowExport] = useState(false);                        // 导出粒度弹窗
  const [expScope, setExpScope] = useState<"all" | "current" | "range">("all");
  const [expFrom, setExpFrom] = useState(1);
  const [expTo, setExpTo] = useState(1);
  const [expMode, setExpMode] = useState<"all" | "narr" | "chat">("all");
  const [expFmt, setExpFmt] = useState<"txt" | "md" | "html">("txt");
  const [pView, setPView] = useState<"grid" | "list" | "masonry" | "group">("grid");   // 立绘素材视图
  const [pSel, setPSel] = useState<Set<number>>(new Set());
  const [pDetail, setPDetail] = useState<number | null>(null);
  const [pSearch, setPSearch] = useState("");
  const [pRole, setPRole] = useState("全部");
  const [pSort, setPSort] = useState<"date" | "name">("date");
  const [portraits, setPortraits] = useState<PortraitItem[]>([]);
  const [pModal, setPModal] = useState<"import" | "create" | null>(null);
  const [pDraft, setPDraft] = useState({ srcPath: "", name: "", role: "", ver: "v1", tags: "", ratio: "", group: "" });
  const loadPortraits = async () => {
    try { setPortraits(await invoke<PortraitItem[]>("portrait_list")); } catch (e) { console.error(e); setMsg("❌ 加载素材失败: " + String(e)); }
  };
  useEffect(() => { loadPortraits(); }, []);
  const doPick = async (preRole = "", preGroup = "") => {
    try {
      const sel = await open({ multiple: false, filters: [{ name: "图片", extensions: ["png", "jpg", "jpeg", "webp", "bmp", "gif"] }] });
      if (!sel) return;
      const p = Array.isArray(sel) ? sel[0] : sel;
      const base = p.split(/[\\/]/).pop() || "素材";
      const nm = base.replace(/\.[^.]+$/, "");
      setPDraft({ srcPath: p, name: nm, role: preRole, ver: "v1", tags: "", ratio: "", group: preGroup });
      setPModal("import");
    } catch (e) { setMsg("❌ 打开文件对话框失败: " + String(e)); }
  };
  const submitPModal = async () => {
    if (!pDraft.name.trim()) { setMsg("⚠️ 素材名称不能为空"); return; }
    try {
      if (pModal === "import") {
        await invoke("portrait_import", { srcPath: pDraft.srcPath, name: pDraft.name.trim(), role: pDraft.role.trim(), ver: pDraft.ver.trim() || "v1", tags: pDraft.tags.trim(), group: pDraft.group.trim() });
        setMsg("✅ 已导入立绘素材");
      } else {
        await invoke("portrait_create", { name: pDraft.name.trim(), role: pDraft.role.trim(), ver: pDraft.ver.trim() || "v1", ratio: pDraft.ratio.trim(), tags: pDraft.tags.trim() });
        setMsg("✅ 已新建素材（无图，稍后可导入图片）");
      }
      setPModal(null);
      await loadPortraits();
    } catch (e) { setMsg("❌ " + String(e)); }
  };
  const [pBindOpen, setPBindOpen] = useState(false);
  const [bindStory, setBindStory] = useState(0);
  const [bindChar, setBindChar] = useState(0);
  const [bindChars, setBindChars] = useState<{ id: number; name: string }[]>([]);
  const loadBindChars = async (sid: number) => {
    if (!sid) { setBindChars([]); return; }
    try { setBindChars(await invoke<{ id: number; name: string }[]>("list_characters", { storyId: String(sid) })); } catch (e) { setBindChars([]); }
  };
  const doBind = async () => {
    if (!pDetail) return;
    if (!bindStory || !bindChar) { setMsg("⚠️ 请选择故事和角色"); return; }
    try {
      await invoke("portrait_bind", { id: pDetail, storyId: bindStory, characterId: bindChar });
      setMsg("✅ 已绑定角色");
      setPBindOpen(false);
      await loadPortraits();
    } catch (e) { setMsg("❌ " + String(e)); }
  };
  const doUnbind = async () => {
    if (!pDetail) return;
    try {
      await invoke("portrait_bind", { id: pDetail, storyId: null, characterId: null });
      setMsg("🟡 已解除绑定");
      setPBindOpen(false);
      await loadPortraits();
    } catch (e) { setMsg("❌ " + String(e)); }
  };
  const [pGroupOpen, setPGroupOpen] = useState(false);
  const [pGroupInput, setPGroupInput] = useState("");
  const [pTagOpen, setPTagOpen] = useState(false);
  const [pTagInput, setPTagInput] = useState("");
  const [pDelOpen, setPDelOpen] = useState(false);
  const doGroupSet = async () => {
    if (!pGroupInput.trim()) { setMsg("⚠️ 请输入或选择分组名"); return; }
    try {
      const n = await invoke<number>("portrait_group_set", { ids: Array.from(pSel), group: pGroupInput.trim() });
      setMsg(`✅ 已移动 ${n} 个素材到分组「${pGroupInput.trim()}」`);
      setPGroupOpen(false); setPGroupInput(""); setPSel(new Set());
      await loadPortraits();
    } catch (e) { setMsg("❌ " + String(e)); }
  };
  const doTagBatch = async () => {
    if (!pTagInput.trim()) { setMsg("⚠️ 请输入标签"); return; }
    try {
      const n = await invoke<number>("portrait_tag_batch", { ids: Array.from(pSel), tags: pTagInput.trim() });
      setMsg(`✅ 已为 ${n} 个素材追加标签`);
      setPTagOpen(false); setPTagInput(""); setPSel(new Set());
      await loadPortraits();
    } catch (e) { setMsg("❌ " + String(e)); }
  };
  const doPExport = async () => {
    try {
      setMsg(await invoke<string>("portrait_export", { ids: Array.from(pSel) }));
    } catch (e) { setMsg("❌ " + String(e)); }
  };
  const doDelete = async () => {
    try {
      const n = await invoke<number>("portrait_delete", { ids: Array.from(pSel) });
      setMsg(`🗑 已删除 ${n} 个素材`);
      setPDelOpen(false); setPSel(new Set());
      await loadPortraits();
    } catch (e) { setMsg("❌ " + String(e)); }
  };
  const [pRoleEditOpen, setPRoleEditOpen] = useState(false);
  const [pRoleEditVal, setPRoleEditVal] = useState("");
  const doRoleSave = async () => {
    if (!pDetail) return;
    try {
      await invoke("portrait_update", { id: pDetail, role: pRoleEditVal });
      setMsg("✅ 角色已更新");
      setPRoleEditOpen(false);
      await loadPortraits();
    } catch (e) { setMsg("❌ " + String(e)); }
  };
  const [pGenEditOpen, setPGenEditOpen] = useState(false);
  const [pGenForm, setPGenForm] = useState({ prompt: "", negative: "", seed: "", steps: "", cfg: "", model: "" });
  // ===== 生成参数页（左侧导航：立绘素材下方）=====
  const [paramsTab, setParamsTab] = useState<"budget" | "preview" | "image" | "json">("budget");
  const [ctxWindow, setCtxWindow] = useState(32768);
  // 上下文预算（A 子项）：设置页三个输入 + 本地即时校验
  const [ctxWindowInput, setCtxWindowInput] = useState("32768");
  const [reserveRatio, setReserveRatio] = useState("0.8");
  const [maxTokensInput, setMaxTokensInput] = useState("4000");
  const [nRecentInput, setNRecentInput] = useState("10");   // 每日上限：N 的封顶值
  const cwNumber = Number(ctxWindowInput) || 0;
  const cwWarn = cwNumber > 0 && cwNumber < 32768;
  const cwInvalid = ctxWindowInput.trim() !== "" && (!cwNumber || cwNumber <= 0);
  const [pParamId, setPParamId] = useState<number | null>(null);
  const [pDebugId, setPDebugId] = useState<number | null>(null);
  const [pParamForm, setPParamForm] = useState({ prompt: "", negative: "", seed: "", steps: "", cfg: "", model: "" });
  const doGenSave = async () => {
    if (!pDetail) return;
    try {
      await invoke("portrait_update_params", { id: pDetail, prompt: pGenForm.prompt, negative: pGenForm.negative, seed: pGenForm.seed, steps: pGenForm.steps, cfg: pGenForm.cfg, model: pGenForm.model });
      setMsg("✅ 生成参数已更新");
      setPGenEditOpen(false);
      await loadPortraits();
    } catch (e) { setMsg("❌ " + String(e)); }
  };
  const [settingsTab, setSettingsTab] = useState<"api" | "debug" | "data">("api");
  // sub = 故事列表下的二级入口（展开后缩进显示）
  const navItems: { icon: string; label: string; view: "chars" | "world" | "proposals" | "timeline" | "stories" | null; action: (() => void) | null; sub?: boolean }[] = [
    { icon: "📁", label: "故事列表", view: "stories", action: () => { setCurrentStory(null); setChars([]); setView("stories"); } },
    { icon: "📅", label: "时间线", view: "timeline", action: null, sub: true },
    { icon: "👥", label: "角色管理", view: "chars", action: null, sub: true },
    { icon: "🌍", label: "世界观设定", view: "world", action: null, sub: true },
    { icon: "📋", label: "演进提案", view: "proposals", action: null, sub: true },
  ];
  const [worlds, setWorlds] = useState<WorldSetting[]>([]);
  const [worldTab, setWorldTab] = useState<"entries" | "map" | "factions" | "terms" | "conflicts">("map");
  const [worldLoading, setWorldLoading] = useState(false);
  const [worldConflicts, setWorldConflicts] = useState<WorldConflict[]>([]);
  const [worldSelId, setWorldSelId] = useState<number | null>(null);
  const [worldSearch, setWorldSearch] = useState("");
  const [wEditId, setWEditId] = useState<number | null>(null);
  const [wDraft, setWDraft] = useState<{ key: string; value: string; category: string; hard: boolean; meta: Record<string, string> }>({ key: "", value: "", category: "设定", hard: false, meta: {} });
  const [facRelEditId, setFacRelEditId] = useState<number | null>(null);
  const [facRelDraft, setFacRelDraft] = useState({ from: "", to: "", relation: "中立" });

  // ===== 剧情（工作区内）=====
  const [node, setNode] = useState<{ id: number; date_label: string } | null>(null);
  const [timelineNodes, setTimelineNodes] = useState<TimelineNodeDto[]>([]);
  const [messages, setMessages] = useState<{ id: number; role: string; content: string; mode: string; created_at: string }[]>([]);
  const [input, setInput] = useState("");
  const [editingId, setEditingId] = useState<number | null>(null);
  const [editText, setEditText] = useState("");

  // ===== 关系（工作区内）=====
  const [relations, setRelations] = useState<Relation[]>([]);
  const [rFrom, setRFrom] = useState<number>(0);
  const [rTo, setRTo] = useState<number>(0);
  const [rType, setRType] = useState("陌生人");
  const [rDir, setRDir] = useState<"单向" | "双向">("单向");
  const [rStrength, setRStrength] = useState(50);
  const [rState, setRState] = useState("稳定");
  const [rDescription, setRDescription] = useState("");
  const [rEvidence, setREvidence] = useState("");
  const [relEditingId, setRelEditingId] = useState<number | null>(null);
  const [relExpandId, setRelExpandId] = useState<number | null>(null);
  // 关系图谱父节点（null = 自动取焦点角色/第一个角色）
  const [graphRootId, setGraphRootId] = useState<number | null>(null);
  // 当前角色卡焦点角色（null = 默认第一个有状态的角色；点击右侧立绘区才切换）
  const [focusCharId, setFocusCharId] = useState<number | null>(null);

  // ===== 设定演进提案（M6）=====
  type Proposal = {
    id: number; group_id: number; target_type: string; target_id: number;
    field: string; old_value: string | null; new_value: string | null;
    reason: string; evidence: string; confidence: number;
    status: string; note: string; created_at: string;
  };
  const [proposals, setProposals] = useState<Proposal[]>([]);

  const loadProposals = async () => {
    try {
      const list = await invoke<Proposal[]>("list_proposals", { storyId: String(currentStory?.id) });
      setProposals(list);
      await loadEventLogs();
    } catch (e) { console.error(e); }
  };

  const analyzeCurrent = async () => {
    if (!node) { setMsg("⚠️ 请先选中一个剧情节点"); return; }
    try {
      const rep = await invoke<{ events: { event_type: string; matched: string; confidence: number }[]; proposals: Proposal[]; applied_groups: number[]; notes: string[] }>(
        "analyze_evolution", { storyId: String(currentStory?.id), nodeId: node.id }
      );
      const evs = rep.events.map((e) => e.event_type).join(", ");
      setMsg(`✅ 分析完成：事件[${evs || "无"}]，提案 ${rep.proposals.length} 条，自动应用 ${rep.applied_groups.length} 组` + (rep.notes.length ? "；" + rep.notes.join("；") : ""));
      await loadProposals();
      await loadRelations(currentStory!.id);
      await loadChars(currentStory!.id);
    } catch (e) { setMsg("❌ 分析失败: " + String(e)); }
  };

  const confirmGroup = async (gid: number) => {
    try {
      const r = await invoke<string>("confirm_proposal_group", { storyId: String(currentStory?.id), groupId: gid });
      setMsg("✅ " + r);
      await loadProposals();
      await loadRelations(currentStory!.id);
    } catch (e) { setMsg("❌ " + String(e)); }
  };

  const rejectGroup = async (gid: number) => {
    try {
      const r = await invoke<string>("reject_proposal_group", { storyId: String(currentStory?.id), groupId: gid });
      setMsg("🟡 " + r);
      await loadProposals();
    } catch (e) { setMsg("❌ " + String(e)); }
  };

  // ===== M7：快照 / 审计 / 事件日志 / 缓存 =====
  type Snapshot = { id: number; version: number; parent_version: number | null; created_at: string };
  type AuditLog = { id: number; action: string; target: string | null; before: string | null; after: string | null; operator: string; created_at: string };
  type EventLogItem = { id: number; node_id: number | null; event_type: string; payload_json: string; confidence: number; source: string };
  type CacheEntry = { path: string; size: number; type_: string; last_access: string };
  const [snapshots, setSnapshots] = useState<Snapshot[]>([]);
  const [auditLogs, setAuditLogs] = useState<AuditLog[]>([]);
  const [eventLogs, setEventLogs] = useState<EventLogItem[]>([]);
  const [cacheList, setCacheList] = useState<CacheEntry[]>([]);
  const [cacheUsageBytes, setCacheUsageBytes] = useState(0);
  const [cacheLimit, setCacheLimit] = useState(5);

  const loadSnapshots = async () => {
    try { setSnapshots(await invoke<Snapshot[]>("list_snapshots", { storyId: String(currentStory?.id) })); } catch (e) { console.error(e); }
  };
  const loadAudit = async () => {
    try { setAuditLogs(await invoke<AuditLog[]>("list_audit_logs", { storyId: String(currentStory?.id) })); } catch (e) { console.error(e); }
  };
  const loadEventLogs = async () => {
    try { setEventLogs(await invoke<EventLogItem[]>("list_event_logs", { storyId: String(currentStory?.id) })); } catch (e) { console.error(e); }
  };
  const loadCache = async () => {
    try {
      setCacheList(await invoke<CacheEntry[]>("list_cache_entries", { storyId: String(currentStory?.id) }));
      setCacheUsageBytes(await invoke<number>("cache_usage", { storyId: String(currentStory?.id) }));
      setCacheLimit(await invoke<number>("get_cache_limit"));
    } catch (e) { console.error(e); }
  };

  const rollbackTo = async (v: number) => {
    try {
      const r = await invoke<string>("rollback_snapshot", { storyId: String(currentStory?.id), version: v });
      setMsg("🔄 " + r);
      await loadSnapshots(); await loadAudit(); await loadRelations(currentStory!.id); await loadChars(currentStory!.id); await loadWorlds(currentStory!.id);
    } catch (e) { setMsg("❌ " + String(e)); }
  };
  const clearCacheNow = async () => {
    try { setMsg(await invoke<string>("clear_cache", { storyId: String(currentStory?.id) })); await loadCache(); } catch (e) { setMsg("❌ " + String(e)); }
  };
  const autoClean = async () => {
    try { setMsg(await invoke<string>("auto_cleanup_cache", { storyId: String(currentStory?.id) })); await loadCache(); } catch (e) { setMsg("❌ " + String(e)); }
  };
  const saveCacheLimit = async () => {
    try { setMsg(await invoke<string>("set_cache_limit", { gb: cacheLimit })); } catch (e) { setMsg("❌ " + String(e)); }
  };

  const proposalRelationLabel = (p: Proposal) => {
    const rel = relations.find((r) => r.id === p.target_id);
    if (!rel) return `关系 #${p.target_id}`;
    return `${charNameOf(rel.from_character_id)} ${rel.direction === "双向" ? "⟷" : "→"} ${charNameOf(rel.to_character_id)}`;
  };

  const loadStories = async () => {
    try {
      const list = await invoke<Story[]>("list_stories");
      setStories(list);
    } catch (e) {
      setMsg("❌ 加载故事失败: " + String(e));
    }
  };

  useEffect(() => {
    loadStories();
    loadSettings();
  }, []);

  const loadSettings = async () => {
    try {
      const bu = await invoke<string | null>("get_config", { key: "model.base_url" });
      const mn = await invoke<string | null>("get_config", { key: "model.name" });
      setBaseUrl(bu ?? "");
      setModelName(mn ?? "");
      const cw = await invoke<string | null>("get_config", { key: "model.context_window" });
      if (cw) { setCtxWindowInput(cw); const n = Number(cw); if (n > 0) setCtxWindow(n); }
      const rr = await invoke<string | null>("get_config", { key: "model.context_reserve_ratio" });
      if (rr) setReserveRatio(rr);
      const mt = await invoke<string | null>("get_config", { key: "model.max_tokens" });
      if (mt) setMaxTokensInput(mt);
      const nr = await invoke<string | null>("get_config", { key: "context.n_recent" });
      if (nr) setNRecentInput(nr);
    } catch (e) {
      console.error(e);
    }
  };

  const saveSettings = async () => {
    try {
      await invoke("set_config", { key: "model.base_url", value: baseUrl });
      await invoke("set_config", { key: "model.name", value: modelName });
      // 上下文预算三项：窗口 / 系数 / 单次最大输出（安全余量保持默认 800，一般不用改）
      const cw = cwNumber;
      if (cw > 0) {
        await invoke("set_config", { key: "model.context_window", value: String(cw) });
        setCtxWindow(cw);
      }
      const rr = Number(reserveRatio);
      if (rr > 0 && rr <= 1) await invoke("set_config", { key: "model.context_reserve_ratio", value: String(rr) });
      const mt = Number(maxTokensInput);
      if (mt > 0) await invoke("set_config", { key: "model.max_tokens", value: String(mt) });
      const nr = Number(nRecentInput);
      if (nr > 0) await invoke("set_config", { key: "context.n_recent", value: String(nr) });
      if (apiKeyInput) {
        await invoke("set_api_key", { account: "openai_api_key", key: apiKeyInput });
        setApiKeyInput("");
      }
      setMsg("✅ 设置已保存");
    } catch (e) {
      setMsg("❌ 保存失败: " + String(e));
    }
  };

  const loadDebug = async () => {
    if (!currentStory) return;
    try {
      const list = await invoke<DebugReq[]>("list_debug_requests", { storyId: String(currentStory.id) });
      setDebugList(list);
    } catch (e) {
      setMsg("❌ 加载快照失败: " + String(e));
    }
  };

  const clearDebug = async () => {
    if (!currentStory) return;
    if (!confirm("清空所有请求快照？")) return;
    try {
      await invoke("clear_debug_requests", { storyId: String(currentStory.id) });
      setDebugList([]);
      setSelectedDebug(null);
      setMsg("🗑️ 已清空快照");
    } catch (e) {
      setMsg("❌ 清空失败: " + String(e));
    }
  };

  const runTest = async () => {
    setTestBusy(true);
    setTestResult(null);
    try {
      const r = await invoke<{ ok: boolean; detail: string; reply?: string }>("test_api_connection");
      setTestResult(r);
    } catch (e) {
      setTestResult({ ok: false, detail: "调用失败: " + String(e) });
    } finally {
      setTestBusy(false);
    }
  };

  const createStory = async () => {
    if (!newTitle.trim()) {
      setMsg("❌ 标题不能为空");
      return;
    }
    try {
      await invoke("create_story", { title: newTitle.trim(), timeOrigin: newOrigin });
      setMsg(`✅ 已创建「${newTitle.trim()}」`);
      setShowNew(false);
      setNewTitle("");
      await loadStories();
    } catch (e) {
      setMsg("❌ 创建失败: " + String(e));
    }
  };

  const removeStory = async (id: number, title: string) => {
    if (!confirm(`确定删除故事「${title}」？其 story.db 也会被删除。`)) return;
    try {
      await invoke("delete_story", { id });
      setMsg(`🗑️ 已删除「${title}」`);
      await loadStories();
    } catch (e) {
      setMsg("❌ 删除失败: " + String(e));
    }
  };

  // ===== D 子项：日摘要 =====
  // ① 补齐"历史节点"的日摘要（幂等）：只处理当前节点之前、有剧情、还没摘要的天；当前节点不生成。
  // ② 当天到量（AI 回复 ≥ 阈值）时自动生成/刷新**当天**摘要 —— 用户 2026-10-11 定的规则。
  // 失败必须可见（曾经静默吞掉，导致"什么都没发生"无法定位）；但失败不打断写作。
  const SUMMARY_MIN_REPLIES = 10;
  const refreshTimelineNodes = async (storyId: string) => {
    const nodes = await invoke<TimelineNodeDto[]>("list_timeline_nodes", { storyId });
    setTimelineNodes(nodes);
  };
  const backfillSummaries = async (storyId: string, currentNodeId: number) => {
    try {
      const rep = await invoke<{ done: string[]; failed: string[] }>("backfill_day_summaries", { storyId, currentNodeId });
      if (rep?.done?.length) setMsg(`📝 已为 ${rep.done.join("、")} 生成日摘要`);
      if (rep?.failed?.length) setMsg(`⚠ 日摘要失败：${rep.failed.join("；")}`);
      if (rep?.done?.length || rep?.failed?.length) await refreshTimelineNodes(storyId);
    } catch (e) {
      setMsg("⚠ 日摘要补齐调用失败：" + String(e));
      console.error("日摘要补齐失败：", e);
    }
  };
  // 当天到量自动摘要（阈值：AI 回复条数）
  const autoSummarizeToday = async (storyId: string, nodeId: number) => {
    try {
      const made = await invoke<string | null>("maybe_auto_day_summary", { storyId, nodeId, minReplies: SUMMARY_MIN_REPLIES });
      if (made) {
        setMsg(`📝 ${made}，已自动生成当天日摘要`);
        await refreshTimelineNodes(storyId);
      }
    } catch (e) {
      setMsg("⚠ 当天日摘要生成失败：" + String(e));
      console.error(e);
    }
  };

  // ===== 进入工作区 / 角色 =====
  const enterStory = async (s: Story) => {
    setCurrentStory(s);
    setDebugList([]);
    setSelectedDebug(null);
    setCharGraphRootId(null);
    setMsg("");
    setView("timeline");
    await loadChars(s.id);
    await loadWorlds(s.id);
    await loadRelations(s.id);
    await loadTimelineAndMessages(s.id);
  };

  const loadTimelineAndMessages = async (storyId: number, nodeId?: number) => {
    try {
      const nodes = await invoke<TimelineNodeDto[]>(
        "list_timeline_nodes",
        { storyId: String(storyId) }
      );
      if (nodes.length === 0) {
        // 没有节点则建第 1 天
        const n = await invoke<{ id: number; date_label: string }>("ensure_first_node", {
          storyId: String(storyId),
        });
        setNode(n);
        setTimelineNodes([{ id: n.id, date_label: n.date_label, sort_order: 1, node_type: "day", is_flashback: 0, tags: "", day_summary: "" }]);
        setMessages([]);
        return;
      }
      setTimelineNodes(nodes);
      const target = nodeId != null ? nodes.find((n) => n.id === nodeId) : nodes[nodes.length - 1];
      if (!target) return;
      setNode({ id: target.id, date_label: target.date_label });
      const list = await invoke<{ id: number; role: string; content: string; mode: string; created_at: string }[]>(
        "list_messages",
        { storyId: String(storyId), nodeId: target.id }
      );
      setMessages(list);
      // D 子项：切到某天时，顺手补齐更早那些"有剧情但还没摘要"的天（幂等、异步、不阻塞界面）
      backfillSummaries(String(storyId), target.id);
    } catch (e) {
      setMsg("❌ 加载时间线失败: " + String(e));
    }
  };

  const switchNode = async (nodeId: number) => {
    if (!currentStory) return;
    await loadTimelineAndMessages(currentStory.id, nodeId);
  };

  // D 子项：手动生成"当前这天"的日摘要（已有则询问是否覆盖 —— 摘要一旦生成就不回改，覆盖需用户点头）
  const makeDaySummary = async (force = false) => {
    if (!currentStory || !node) return;
    try {
      setMsg("📝 正在生成日摘要…");
      const s = await invoke<string | null>("generate_day_summary", {
        storyId: String(currentStory.id), nodeId: node.id, force,
      });
      if (s) {
        setMsg("📝 日摘要已生成");
        await loadTimelineAndMessages(currentStory.id, node.id);
      } else {
        setMsg("ℹ 这天已有摘要（生成一次即固化，不再回改）");
      }
    } catch (e) {
      setMsg("❌ 日摘要失败: " + String(e));
    }
  };

  const addNextDay = async () => {
    if (!currentStory) return;
    try {
      const n = await invoke<{ id: number; date_label: string }>("create_next_day_node", {
        storyId: String(currentStory.id),
      });
      setMsg(`✅ 已创建 ${n.date_label}`);
      await loadTimelineAndMessages(currentStory.id, n.id);
    } catch (e) {
      setMsg("❌ 创建下一天失败: " + String(e));
    }
  };

  const startEdit = (m: { id: number; content: string }) => {
    setEditingId(m.id);
    setEditText(m.content);
  };

  const cancelEdit = () => {
    setEditingId(null);
    setEditText("");
  };

  const saveEdit = async () => {
    if (!currentStory || editingId == null) return;
    try {
      await invoke("update_message", {
        storyId: String(currentStory.id),
        messageId: editingId,
        content: editText,
      });
      setMsg("✅ 已修改历史剧情（后续续写将基于修改后的内容）");
      cancelEdit();
      if (node) await loadTimelineAndMessages(currentStory.id, node.id);
    } catch (e) {
      setMsg("❌ 修改失败: " + String(e));
    }
  };

  const addInterlude = async () => {
    if (!currentStory) return;
    try {
      const n = await invoke<{ id: number; date_label: string }>("create_interlude_node", {
        storyId: String(currentStory.id),
      });
      setMsg(`🎭 已创建插曲：${n.date_label}`);
      await loadTimelineAndMessages(currentStory.id, n.id);
    } catch (e) {
      setMsg("❌ 创建插曲失败: " + String(e));
    }
  };

  const removeNode = async (nodeId: number, label: string) => {
    if (!currentStory) return;
    if (!confirm(`删除「${label}」节点及其所有消息？`)) return;
    try {
      await invoke("delete_timeline_node", {
        storyId: String(currentStory.id),
        nodeId,
      });
      setMsg(`🗑️ 已删除 ${label}`);
      await loadTimelineAndMessages(currentStory.id);
    } catch (e) {
      setMsg("❌ 删除节点失败: " + String(e));
    }
  };

  const updateConstraints = (list: string[]) => { setConstraints(list); localStorage.setItem("ai.constraints", JSON.stringify(list)); };
  const updateDirectors = (list: string[]) => { setDirectors(list); localStorage.setItem("ai.directors", JSON.stringify(list)); };

  const doExport = async () => {
    if (!currentStory) return;
    let ids: number[] = [];
    let label = "";
    if (expScope === "current") {
      if (!node) { setMsg("⚠ 当前没有选中的节点，请先在时间线或剧情页选择节点"); return; }
      ids = [node.id];
      label = "当前节点";
    } else if (expScope === "range") {
      const lo = Math.min(expFrom, expTo), hi = Math.max(expFrom, expTo);
      ids = dayNodes.filter((n) => { const d = dayNoOf(n); return d >= lo && d <= hi; }).map((n) => n.id);
      label = `第 ${lo} 天 ~ 第 ${hi} 天（不含插曲）`;
      if (ids.length === 0) { setMsg("⚠ 该区间内没有节点"); return; }
    } else {
      label = "全部节点";
    }
    const modeLabel = expMode === "narr" ? "仅旁白" : expMode === "chat" ? "仅对话" : "全部";
    const cmd = expFmt === "md" ? "export_story_md" : expFmt === "html" ? "export_story_html" : "export_story_txt";
    const fmtLabel = expFmt === "md" ? "MD" : expFmt === "html" ? "HTML" : "TXT";
    setMsg(`⏳ 正在导出 ${fmtLabel}…`);
    try {
      const r = await invoke(cmd, { storyId: String(currentStory.id), nodeIds: ids, modeFilter: expMode, scopeLabel: `${label} · 模式 ${modeLabel}` });
      setMsg(`✅ 已导出 ${fmtLabel}：` + String(r));
      setShowExport(false);
    } catch (e) { setMsg("❌ 导出失败：" + String(e)); }
  };

  const sendMessage = async () => {
    if (!currentStory || !node || !input.trim()) return;
    const text = input.trim();
    if (sensitiveFilter) {
      const hit = SENSITIVE_WORDS.find((w) => text.includes(w));
      if (hit) { setMsg("🛡 输入包含敏感词，已拦截：" + hit); return; }
    }
    // 导演指令：仅沉浸对话模式，一次性——附加到本条消息开头，发完清空
    const pendingDirectors = navMode === "对话" ? directors.filter((d) => d.trim()) : [];
    if (!text && pendingDirectors.length === 0) return;
    // 解析 #标签（元指令，从剧情正文中剥离）与 @角色（本次聚焦，仅注入提示词）
    const tagRe = /#([^\s#，。、！？]+)/g;
    const tags = (text.match(tagRe) || []).map((t) => t.slice(1)).join(",");
    const stripped = tags ? text.replace(tagRe, "").replace(/\s+/g, " ").trim() : text;
    const focus = text.match(/@([^\s@，。、！？]+)/)?.[1] || "";
    // 指令在正文之前，用空行分隔（后端按同一前缀解析并剥离）
    const content = pendingDirectors.length > 0
      ? `${DIRECTOR_TAG}${pendingDirectors.join("\n")}${stripped ? "\n\n" + stripped : ""}`
      : stripped;
    setInput("");
    if (pendingDirectors.length > 0) updateDirectors([]);   // 一次性：发完即清空待发列表
    try {
      // 存用户消息
      await invoke("create_message", {
        storyId: String(currentStory.id),
        nodeId: node.id,
        role: "user",
        content,
        mode: navMode === "旁白" ? "narr" : "chat", // 用户消息记录发送时模式，导出按模式过滤
      });
      await loadTimelineAndMessages(currentStory.id, node.id);

      // 准备一个空的 AI 气泡，逐字追加
      const placeholderId = -Date.now();
      setMessages((prev) => [...prev, {
        id: placeholderId,
        node_id: node.id,
        role: "assistant",
        content: "",
        mode: "chat",
        created_at: new Date().toLocaleString(),
      }]);

      // 监听流式事件
      const unlistenToken = await listen<{ delta: string }>("chat-token", (e) => {
        let delta = e.payload.delta;
        if (sensitiveFilter) delta = SENSITIVE_WORDS.reduce((s, w) => s.split(w).join("***"), delta);
        setMessages((prev) => prev.map((m) =>
          m.id === placeholderId ? { ...m, content: m.content + delta } : m
        ));
      });
      const unlistenDone = await listen("chat-done", async () => {
        unlistenToken();
        unlistenDone();
        unlistenError();
        await loadTimelineAndMessages(currentStory.id, node.id);
        await loadChars(currentStory.id); // M5: 回复后刷新角色状态卡
        await loadDebug(); // 刷新当前节点 Token 预算快照
        // D 子项：回复完成后 ① 补齐更早那些天 ② 当天到量则自动刷当天摘要（都不阻塞界面）
        backfillSummaries(String(currentStory.id), node.id).then(() =>
          autoSummarizeToday(String(currentStory.id), node.id)
        );
      });
      const unlistenError = await listen<{ detail: string }>("chat-error", (e) => {
        unlistenToken();
        unlistenDone();
        unlistenError();
        setMessages((prev) => prev.filter((m) => m.id !== placeholderId));
        setMsg("❌ " + e.payload.detail);
      });

      // 调流式 API
      await invoke("generate_story_reply_stream", {
        storyId: String(currentStory.id),
        nodeId: node.id,
        userText: content,
        mode: navMode === "旁白" ? "narr" : "chat",
        myRole,
        aiRole,
        focusRole: focus,
        tags,
        constraints: constraints.map((cst, i) => `${i + 1}. ${cst}`).join("\n"),
        lengthLevel: lenLevel,
        memory: memoryCtx,
      });
    } catch (e) {
      setMsg("❌ 发送失败: " + String(e));
    }
  };

  const loadChars = async (storyId: number) => {
    setCharsLoading(true);
    try {
      const [list, usage] = await Promise.all([
        invoke<Character[]>("list_characters", { storyId: String(storyId) }),
        invoke<CharacterUsage[]>("character_usage", { storyId: String(storyId) }),
      ]);
      setChars(list);
      setCharUsage(usage);
    } catch (e) {
      setMsg("❌ 加载角色失败: " + String(e));
    } finally {
      setCharsLoading(false);
    }
  };

  // 「我扮演 / AI 扮演」默认值（每个故事只应用一次）：我扮演=男主（没有男主则保持自由），AI 扮演=女主
  const roleDefaultStory = useRef<number | null>(null);
  useEffect(() => {
    if (!currentStory || chars.length === 0) return;
    if (roleDefaultStory.current === currentStory.id) return;
    roleDefaultStory.current = currentStory.id;
    const mine = chars.find((c) => c.role === "男主") ?? null;
    const hers = chars.find((c) => c.role === "女主") ?? null;
    if (mine && myRole === "自由") setMyRole(mine.name);
    const aiPick = hers ?? chars.find((c) => c.name !== (mine?.name ?? "")) ?? null;
    if (aiPick && aiRole === "自由") setAiRole(aiPick.name);
  }, [chars, currentStory?.id]);

  // 扮演角色校验：状态里记的「上次校验过的组合」，避免同一组合重复提示
  const roleFixKey = useRef<string>("");

  // 切换 / 进入故事时重置两个「只跑一次」的标记，让默认值重算、扮演角色重新校验
  useEffect(() => {
    roleDefaultStory.current = null;
    roleFixKey.current = "";
  }, [currentStory?.id]);

  // 扮演角色校验：我扮演 / AI 扮演是全局状态，不随故事重置，很容易把上一个故事的角色（或已被删除的角色）带进来；
  // 这里每次进入故事或角色表变化时都校验一遍：不在本故事角色表里就按默认规则纠正
  const namesKey = chars.map((c) => c.name).join("\u0001");
  const roleCheckKey = `${currentStory?.id ?? 0}|${charsLoading ? 1 : 0}|${namesKey}|${myRole}|${aiRole}`;
  useEffect(() => {
    if (!currentStory || charsLoading) return;
    if (roleFixKey.current === roleCheckKey) return;
    roleFixKey.current = roleCheckKey;
    const names = new Set(chars.map((c) => c.name));
    const fixed: string[] = [];
    if (myRole !== "自由" && !names.has(myRole)) {
      const mine = chars.find((c) => c.role === "男主") ?? null;
      setMyRole(mine ? mine.name : "自由");
      fixed.push(`「我扮演」${myRole}（不在本故事角色表）→ ${mine ? mine.name : "自由"}`);
    }
    if (aiRole !== "自由" && !names.has(aiRole)) {
      const hers = chars.find((c) => c.role === "女主") ?? null;
      setAiRole(hers ? hers.name : "自由");
      fixed.push(`「AI 扮演」${aiRole}（不在本故事角色表）→ ${hers ? hers.name : "自由"}`);
    }
    if (fixed.length > 0) setMsg(`⚠ 扮演角色已纠正：${fixed.join("；")}`);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [roleCheckKey]);

  // M5: 从 current_status 解析主情绪 -> 徽章样式
  const emotionInfo = (status: string): { label: string; color: string; bg: string } => {
    const m = status.match(/情绪：([^｜]+)/);
    const e = m ? m[1].trim() : "";
    const map: Record<string, [string, string]> = {
      "生气": ["#EA6668", "#fdecec"],
      "开心": ["#52C41A", "#eaf7e6"],
      "难过": ["#4A7FD9", "#eaf1fc"],
      "害羞": ["#EAA7B2", "#fdf0f2"],
      "惊讶": ["#FAAD14", "#fef6e6"],
      "害怕": ["#C9A7E8", "#f5eefc"],
      "平静": ["#9E9E9E", "#f2f2f2"],
      "温柔": ["#94D8C3", "#eaf8f3"],
    };
    const hit = map[e];
    return hit ? { label: e, color: hit[0], bg: hit[1] } : { label: e || "", color: "#6f7686", bg: "#f5f5f5" };
  };
  // 剧情日期：time_origin（YYYY-MM-DD）为绝对起点，第 N 天 = 起点 + (N-1) 天，返回 "MM-DD"；插曲/非日期降级为空
  const nodeDateDisplay = (n: { node_type: string; date_label: string }): string => {
    if (n.node_type === "interlude" || n.node_type === "flashback") return "";
    if (!currentStory) return "";
    const m = currentStory.time_origin.trim().match(/^(\d{4})[-./年](\d{1,2})[-./月](\d{1,2})日?$/);
    if (!m) return "";
    const dayNo = parseInt(n.date_label.replace(/\D/g, ""), 10);
    if (!dayNo || dayNo <= 0) return "";
    const base = new Date(+m[1], +m[2] - 1, +m[3]);
    base.setDate(base.getDate() + (dayNo - 1));
    return `${String(base.getMonth() + 1).padStart(2, "0")}-${String(base.getDate()).padStart(2, "0")}`;
  };
  const dayNoOf = (n: { date_label: string }) => parseInt(n.date_label.replace(/\D/g, ""), 10) || 0;   // "第 3 天" → 3
  const dayNodes = timelineNodes.filter((n) => n.node_type !== "interlude" && n.node_type !== "flashback"); // 可导出区间选项（day 节点）
  const loadWorlds = async (storyId: number) => {
    try {
      const list = await invoke<WorldSetting[]>("list_world_settings", {
        storyId: String(storyId),
      });
      setWorlds(list);
    } catch (e) {
      setMsg("❌ 加载世界观失败: " + String(e));
    }
  };

  const loadRelations = async (storyId: number) => {
    try {
      const list = await invoke<Relation[]>("list_relations", {
        storyId: String(storyId),
      });
      setRelations(list);
    } catch (e) {
      setMsg("❌ 加载关系失败: " + String(e));
    }
  };

  const openCharForm = (c: Character | null) => {
    setCharError("");
    if (c) {
      setCharEditingId(c.id);
      setCharForm({
        name: c.name, alias: c.alias, role: c.role, faction: c.faction, age: c.age, gender: c.gender,
        first_appearance: c.first_appearance, one_liner: c.one_liner, profile: c.profile,
        speech_style: c.speech_style, traits: c.traits,
        stat_stamina: c.stat_stamina, stat_sanity: c.stat_sanity, stat_emotion: c.stat_emotion, stat_screen: c.stat_screen,
      });
    } else {
      setCharEditingId(null);
      setCharForm({ ...EMPTY_CHAR_FORM });
    }
    setCharFormOpen(true);
  };

  const saveCharForm = async () => {
    if (!currentStory) return;
    if (!charForm.name.trim()) { setCharError("角色名不能为空"); return; }
    if (!charForm.gender.trim()) { setCharError("性别为必填项，请先填写性别"); return; }
    const input = { ...charForm, name: charForm.name.trim() };
    try {
      if (charEditingId) {
        await invoke("update_character", { storyId: String(currentStory.id), id: charEditingId, input });
        setMsg(`✅ 已更新角色「${input.name}」`);
      } else {
        await invoke("create_character", { storyId: String(currentStory.id), input });
        setMsg(`✅ 已新建角色「${input.name}」`);
      }
      setCharFormOpen(false);
      setCharError("");
      await loadChars(currentStory.id);
    } catch (e) {
      setCharError(String(e));
    }
  };

  const randomCharForm = () => {
    const pick = (arr: string[]) => arr[Math.floor(Math.random() * arr.length)];
    setCharForm((f) => ({
      ...f,
      traits: f.traits || pick(["克制", "偏执", "责任感过重", "冷幽默", "温柔", "警惕", "乐观"]),
      stat_stamina: 30 + Math.floor(Math.random() * 60),
      stat_sanity: 30 + Math.floor(Math.random() * 60),
      stat_emotion: 30 + Math.floor(Math.random() * 60),
      stat_screen: 10 + Math.floor(Math.random() * 80),
    }));
  };

  const removeChar = async (id: number, name: string) => {
    if (!currentStory) return;
    if (!confirm(`删除角色「${name}」？`)) return;
    try {
      await invoke("delete_character", { storyId: String(currentStory.id), id });
      setMsg(`🗑️ 已删除角色「${name}」`);
      await loadChars(currentStory.id);
    } catch (e) {
      setMsg("❌ 删除失败: " + String(e));
    }
  };

  const removeWorld = async (id: number, key: string) => {
    if (!currentStory) return;
    if (!confirm(`删除世界观条目「${key}」？`)) return;
    try {
      await invoke("delete_world_setting", { storyId: String(currentStory.id), id });
      setMsg(`🗑️ 已删除「${key}」`);
      await loadWorlds(currentStory.id);
    } catch (e) {
      setMsg("❌ 删除失败: " + String(e));
    }
  };

  const resetRelationForm = () => {
    setRelEditingId(null);
    setRType("陌生人");
    setRStrength(50);
    setRState("稳定");
    setRDescription("");
    setREvidence("");
  };

  const saveRelation = async () => {
    if (!currentStory) return;
    if (rFrom === 0 || rTo === 0) { setMsg("❌ 请选择两个角色"); return; }
    if (rFrom === rTo) { setMsg("❌ 两个角色不能是同一个"); return; }
    const relationType = rType.trim();
    const state = rState.trim() || "稳定";
    const description = rDescription.trim();
    const evidence = rEvidence.trim();
    try {
      if (relEditingId) {
        await invoke("update_relation", {
          storyId: String(currentStory.id), id: relEditingId,
          relationType, direction: rDir, strength: rStrength, state, description, evidence,
        });
        setMsg("✅ 已更新关系");
      } else {
        await invoke("create_relation", {
          storyId: String(currentStory.id), fromId: rFrom, toId: rTo,
          relationType, direction: rDir, strength: rStrength, state, description, evidence,
        });
        setMsg("✅ 已添加关系");
      }
      resetRelationForm();
      await loadRelations(currentStory.id);
    } catch (e) {
      setMsg((relEditingId ? "❌ 更新关系失败: " : "❌ 添加关系失败: ") + String(e));
    }
  };

  const editRelation = (r: Relation) => {
    setRelEditingId(r.id);
    setRFrom(r.from_character_id);
    setRTo(r.to_character_id);
    setRType(r.relation_type || "陌生人");
    setRDir(r.direction === "双向" ? "双向" : "单向");
    setRStrength(r.strength);
    setRState(normRelState(r.state));
    setRDescription(r.description || "");
    setREvidence(r.evidence || "");
    setCharTab("relations");
  };

  const removeRelation = async (id: number) => {
    if (!currentStory) return;
    if (!confirm("删除这条关系？")) return;
    try {
      await invoke("delete_relation", { storyId: String(currentStory.id), id });
      setMsg("🗑️ 已删除关系");
      await loadRelations(currentStory.id);
    } catch (e) {
      setMsg("❌ 删除失败: " + String(e));
    }
  };

  const charNameOf = (id: number) => chars.find((c) => c.id === id)?.name ?? `#${id}`;

  // ===== 工作区视图 =====
  // ===== 生成参数页逻辑 =====
  const paramPortrait = portraits.find((p) => p.id === pParamId) ?? portraits.find((p) => p.story_id === currentStory?.id) ?? portraits[0] ?? null;
  useEffect(() => {
    if (view !== "params") return;
    if (currentStory) loadDebug();
    invoke<string | null>("get_config", { key: "model.context_window" })
      .then((v) => { const n = Number(v); if (n > 0) setCtxWindow(n); })
      .catch(() => {});
  }, [view, currentStory?.id]);
  useEffect(() => {
    if (view !== "params") return;
    if (debugList.length && !debugList.some((d) => d.id === selectedDebug?.id)) setSelectedDebug(debugList[0]);
  }, [view, debugList, selectedDebug]);
  useEffect(() => {
    if (view !== "params" || !paramPortrait) return;
    setPParamForm({ prompt: paramPortrait.prompt || "", negative: paramPortrait.negative || "", seed: paramPortrait.seed || "", steps: paramPortrait.steps || "", cfg: paramPortrait.cfg || "", model: paramPortrait.model || "" });
  }, [view, paramPortrait?.id]);
  const saveParamPortrait = async () => {
    if (!paramPortrait) return;
    try {
      await invoke("portrait_update_params", { id: paramPortrait.id, prompt: pParamForm.prompt, negative: pParamForm.negative, seed: pParamForm.seed, steps: pParamForm.steps, cfg: pParamForm.cfg, model: pParamForm.model });
      setMsg("✅ 生成参数已保存");
      await loadPortraits();
    } catch (e) { setMsg("❌ " + String(e)); }
  };
  const rollParamSeed = () => setPParamForm((f) => ({ ...f, seed: String(Math.floor(Math.random() * 1000000)) }));
  useEffect(() => {
    if (view === "params") setPDebugId(null);
  }, [view, node?.id]);
  useEffect(() => {
    if (view !== "story" || !currentStory) return;
    loadDebug();
    invoke<string | null>("get_config", { key: "model.context_window" })
      .then((v) => { const n = Number(v); if (n > 0) setCtxWindow(n); })
      .catch(() => {});
  }, [view, currentStory?.id, node?.id]);
  useEffect(() => {
    if (view === "chars" && currentStory) loadChars(currentStory.id);
  }, [view, currentStory?.id]);
  const dayDebug = node ? debugList.find((d) => d.node_id === node.id) ?? null : null;
  const dayBudget = computeBudget(dayDebug, ctxWindow);
  const pendingGroups = new Set(proposals.filter((p) => p.status === "pending").map((p) => p.group_id)).size;
    return (
      <div style={{ display: "flex", height: "100vh", fontFamily: "system-ui, sans-serif", background: T.win, color: T.text1 }}>
          <style>{`::-webkit-scrollbar{width:8px;height:8px}::-webkit-scrollbar-track{background:transparent}::-webkit-scrollbar-thumb{background:rgba(255,255,255,.14);border-radius:4px}::-webkit-scrollbar-thumb:hover{background:rgba(255,255,255,.24)}@keyframes dsh-blink{0%,49%{opacity:1}50%,100%{opacity:0}}button{background:#191c22;color:#a3a9b8;border:1px solid #2e333d;border-radius:6px;font-family:inherit;cursor:pointer}button:hover{color:#fff;border-color:#3d4451}button:disabled{opacity:.5;cursor:not-allowed}input,select,textarea{background:#191c22;color:#e8eaf0;border:1px solid #2e333d;border-radius:6px;font-family:inherit}input:focus,select:focus,textarea:focus{border-color:rgba(255,122,24,.55);outline:none}`}</style>
        {/* ① 左导航 */}
        <div style={{ width: 200, borderRight: `1px solid ${T.line}`, display: "flex", flexDirection: "column", background: T.panel, flexShrink: 0 }}>
          <div style={{ padding: "14px 16px 12px", borderBottom: `1px solid ${T.lineSoft}` }}>
            <div style={{ fontWeight: 700, fontSize: 14, letterSpacing: 0.4 }}>Story-Weaver</div>
            <div style={{ fontSize: 11, color: T.text3, marginTop: 4 }}>剧情导演工作台</div>
          </div>
          <div style={{ flex: 1, padding: "8px", display: "flex", flexDirection: "column", gap: 2, overflowY: "auto" }}>
            {/* 剧情区：故事列表（一级） */}
            {navItems.filter((it) => !it.sub).map((it) => {
              const active = it.view && view === it.view;
              return (
                <button
                  key={it.label}
                  onClick={() => { if (it.action) { it.action(); return; } if (!currentStory) { setView("stories"); return; } setView(it.view as "chars" | "world" | "proposals" | "timeline" | "stories"); }}
                  style={{
                    display: "flex", alignItems: "center", gap: 10, padding: "9px 10px",
                    borderRadius: 8, border: "none", cursor: "pointer", fontSize: 13, textAlign: "left", position: "relative",
                    background: active ? T.accentSoft : "transparent",
                    color: active ? T.accent : T.text2,
                    fontWeight: active ? 600 : 400,
                  }}
                >
                  {active && <span style={{ position: "absolute", left: -8, top: 8, bottom: 8, width: 3, background: T.accent, borderRadius: "0 3px 3px 0" }} />}
                  <span>{it.icon}</span>
                  <span>{it.label}</span>
                </button>
              );
            })}
            {/* 故事列表展开后的二级入口（时间线 / 角色管理 / 世界观设定 / 演进提案）：整体右移，形成层级 */}
            {currentStory && (
              <div style={{ marginLeft: 6, paddingLeft: 8, borderLeft: `1px solid ${T.lineSoft}`, display: "flex", flexDirection: "column", gap: 2 }}>
                {navItems.filter((it) => it.sub).map((it) => {
                  const active = it.view && view === it.view;
                  return (
                    <button
                      key={it.label}
                      onClick={() => { setView(it.view as "chars" | "world" | "proposals" | "timeline"); }}
                      style={{
                        display: "flex", alignItems: "center", gap: 10, padding: "9px 8px",
                        borderRadius: 8, border: "none", cursor: "pointer", fontSize: 13, textAlign: "left", position: "relative",
                        background: active ? T.accentSoft : "transparent",
                        color: active ? T.accent : T.text2,
                        fontWeight: active ? 600 : 400,
                      }}
                    >
                      {active && <span style={{ position: "absolute", left: -7, top: 8, bottom: 8, width: 3, background: T.accent, borderRadius: "0 3px 3px 0" }} />}
                      <span>{it.icon}</span>
                      <span>{it.label}</span>
                      {it.view === "proposals" && pendingGroups > 0 && (
                        <span style={{ marginLeft: "auto", background: T.danger, color: "#fff", fontSize: 10, padding: "1px 6px", borderRadius: 9 }}>{pendingGroups}</span>
                      )}
                    </button>
                  );
                })}
              </div>
            )}
            {/* 立绘素材：开始界面在故事列表下；进入故事后自动落到提案下 */}
            <button
              onClick={() => { setView("portrait"); }}
              style={{
                display: "flex", alignItems: "center", gap: 10, padding: "9px 10px",
                borderRadius: 8, border: "none", cursor: "pointer", fontSize: 13, textAlign: "left", position: "relative",
                background: view === "portrait" ? T.accentSoft : "transparent",
                color: view === "portrait" ? T.accent : T.text2,
                fontWeight: view === "portrait" ? 600 : 400,
              }}
            >
              {view === "portrait" && <span style={{ position: "absolute", left: -8, top: 8, bottom: 8, width: 3, background: T.accent, borderRadius: "0 3px 3px 0" }} />}
              <span>🖼</span>
              <span>立绘素材</span>
            </button>
            {/* 生成参数：位于立绘素材下方 */}
            <button
              onClick={() => { setView("params"); if (currentStory) loadDebug(); }}
              style={{
                display: "flex", alignItems: "center", gap: 10, padding: "9px 10px",
                borderRadius: 8, border: "none", cursor: "pointer", fontSize: 13, textAlign: "left", position: "relative",
                background: view === "params" ? T.accentSoft : "transparent",
                color: view === "params" ? T.accent : T.text2,
                fontWeight: view === "params" ? 600 : 400,
              }}
            >
              {view === "params" && <span style={{ position: "absolute", left: -8, top: 8, bottom: 8, width: 3, background: T.accent, borderRadius: "0 3px 3px 0" }} />}
              <span>🎛</span>
              <span>生成参数</span>
            </button>
            {/* 扩展 / 工程区 */}
            <div style={{ marginTop: 10, paddingTop: 10, borderTop: `1px solid ${T.lineSoft}`, display: "flex", flexDirection: "column", gap: 2 }}>
              <div style={{ fontSize: 10, color: T.text3, padding: "2px 10px 4px", letterSpacing: 0.8 }}>扩展 / 工程</div>
              <button
                onClick={() => setView("plugins")}
                style={{
                  display: "flex", alignItems: "center", gap: 10, padding: "9px 10px",
                  borderRadius: 8, border: "none", cursor: "pointer", fontSize: 13, textAlign: "left", position: "relative",
                  background: view === "plugins" ? T.accentSoft : "transparent",
                  color: view === "plugins" ? T.accent : T.text2,
                  fontWeight: view === "plugins" ? 600 : 400,
                }}
              >
                {view === "plugins" && <span style={{ position: "absolute", left: -8, top: 8, bottom: 8, width: 3, background: T.accent, borderRadius: "0 3px 3px 0" }} />}
                <span>🧩</span>
                <span>插件 / 扩展</span>
              </button>
            </div>
          </div>
          <div style={{ padding: 8, borderTop: `1px solid ${T.lineSoft}` }}>
            <button
              onClick={() => setShowSettings(true)}
              style={{ display: "flex", alignItems: "center", gap: 10, padding: "9px 10px", borderRadius: 8, border: "none", background: "transparent", cursor: "pointer", fontSize: 13, color: T.text2, width: "100%" }}
            >
              ⚙️ 设置
            </button>
          </div>
        </div>

        {/* ② 中间主工作区 */}
        <div style={{ flex: 1, display: "flex", flexDirection: "column", minWidth: 0, background: T.win }}>
          <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: 12, padding: "0 20px", height: 52, borderBottom: `1px solid ${T.line}`, background: T.panel, flexShrink: 0 }}>
            <div>
              {currentStory ? (
                <>
                  <div style={{ fontSize: 14, fontWeight: 600 }}>{currentStory.title}</div>
                  <div style={{ fontSize: 11.5, color: T.text3, marginTop: 2 }}>时间起点：{currentStory.time_origin} · 节点 {timelineNodes.length} · 消息 {messages.length}</div>
                </>
              ) : (
                <>
                  <div style={{ fontSize: 14, fontWeight: 600 }}>{view === "stories" ? "故事列表" : "AI 剧情创作"}</div>
                  <div style={{ fontSize: 11.5, color: T.text3, marginTop: 2 }}>{view === "stories" ? `${stories.length} 个故事 · 点击卡片进入导演工作台` : "从左侧导航选择功能"}</div>
                </>
              )}
            </div>
            <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 8 }}>
              <span style={{ display: "inline-flex", alignItems: "center", gap: 5, height: 26, padding: "0 10px", borderRadius: 13, background: T.panel2, border: `1px solid ${T.line}`, fontSize: 11.5, color: T.text2 }}>
                <span style={{ width: 6, height: 6, borderRadius: "50%", background: T.ok }} />{modelName ? `模型：${modelName}` : "模型未配置"}
              </span>
              <span style={{ display: "inline-flex", alignItems: "center", gap: 5, height: 26, padding: "0 10px", borderRadius: 13, background: T.panel2, border: `1px solid ${T.line}`, fontSize: 11.5, color: T.text2 }}>⚡ 流式输出</span>
              <span style={{ display: "inline-flex", alignItems: "center", gap: 5, height: 26, padding: "0 10px", borderRadius: 13, background: T.panel2, border: `1px solid ${T.line}`, fontSize: 11.5, color: T.text2 }}>💾 已自动保存</span>
              <button onClick={() => { if (!currentStory) return; setShowExport(true); }} style={{ display: "inline-flex", alignItems: "center", gap: 5, height: 26, padding: "0 10px", borderRadius: 13, background: T.panel2, border: `1px solid ${T.line}`, fontSize: 11.5, color: T.text2, cursor: "pointer" }}>📥 导出</button>
              <button onClick={() => { setCurrentStory(null); setChars([]); setView("stories"); }} style={{ padding: "5px 12px", border: `1px solid ${T.line}`, borderRadius: 8, background: T.panel2, color: T.text2, cursor: "pointer", fontSize: 12 }}>← 返回故事库</button>
            </div>
          </div>

          <div style={{ flex: 1, overflowY: "auto", padding: "20px 24px 12px" }}>
            {msg && <p style={{ fontSize: 13, color: T.text2, marginTop: 0 }}>{msg}</p>}

            {/* 故事列表（仅点击左侧"故事列表"时显示；首页为空白）
                宽度自适应：不再限宽，随中间工作区撑满（左右留白各 24px，来自上一层容器 padding） */}
            {(view === "stories") && (
              <div style={{ width: "100%" }}>
                <div style={{ display: "flex", gap: 12, alignItems: "center", marginBottom: 18 }}>
                  <button onClick={() => setShowNew(!showNew)} style={{ height: 34, padding: "0 16px", borderRadius: 8, border: showNew ? `1px solid ${T.line}` : "none", background: showNew ? "#191c22" : "linear-gradient(135deg,#FF7A18,#ff9a45)", color: showNew ? T.text2 : "#231505", fontWeight: showNew ? 400 : 650, cursor: "pointer", fontSize: 13 }}>{showNew ? "取消" : "+ 新建故事"}</button>
                  <button onClick={loadStories} style={{ height: 34, padding: "0 14px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, cursor: "pointer", fontSize: 13 }}>刷新</button>
                  <span style={{ fontSize: 12, color: T.text3 }}>{stories.length} 个故事</span>
                </div>

                {showNew && (
                  <div style={{ border: `1px solid ${T.line}`, padding: 16, borderRadius: 12, marginBottom: 16, background: T.panel }}>
                    <h3 style={{ marginTop: 0, fontSize: 14 }}>故事开幕</h3>
                    <p style={{ fontSize: 12.5, color: T.text3, marginBottom: 12 }}>设定绝对时间起点。起点固定后，"X 天后"由系统自动计算。</p>
                    <div style={{ display: "grid", gridTemplateColumns: "120px 1fr", gap: 8, alignItems: "center" }}>
                      <label style={{ fontSize: 12, color: T.text2 }}>故事标题</label>
                      <input value={newTitle} onChange={(e) => setNewTitle(e.target.value)} placeholder="例如：盛夏的雨" style={{ padding: "7px 10px" }} />
                      <label style={{ fontSize: 12, color: T.text2 }}>时间起点</label>
                      <input value={newOrigin} onChange={(e) => setNewOrigin(e.target.value)} placeholder="2026-01-01 或 架空纪元" style={{ padding: "7px 10px" }} />
                    </div>
                    <div style={{ marginTop: 12 }}>
                      <button onClick={createStory} style={{ height: 32, padding: "0 18px", borderRadius: 8, border: "none", background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505", cursor: "pointer", fontSize: 13, fontWeight: 650 }}>创建</button>
                    </div>
                  </div>
                )}

                <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                  {stories.length === 0 && (
                    <div style={{ border: `1px dashed ${T.line}`, borderRadius: 12, padding: "56px 20px", textAlign: "center", color: T.text3, background: "transparent" }}>
                      <div style={{ fontSize: 26, marginBottom: 10 }}>📚</div>
                      <div style={{ fontSize: 13 }}>还没有故事，点上方"+ 新建故事"开始。</div>
                      <div style={{ fontSize: 11.5, color: T.text3, marginTop: 6 }}>创建后将进入导演工作台（时间线 / 角色 / 世界观 / 演进提案）</div>
                    </div>
                  )}
                  {stories.map((s) => (
                    <div
                      key={s.id}
                      onClick={() => enterStory(s)}
                      style={{
                        border: `1px solid ${T.line}`,
                        padding: "13px 16px",
                        borderRadius: 10,
                        display: "flex",
                        justifyContent: "space-between",
                        alignItems: "center",
                        cursor: "pointer",
                        background: T.panel,
                        transition: "border-color .15s, background .15s",
                      }}
                      onMouseEnter={(e) => { e.currentTarget.style.borderColor = T.accent; e.currentTarget.style.background = "#20242d"; }}
                      onMouseLeave={(e) => { e.currentTarget.style.borderColor = T.line; e.currentTarget.style.background = T.panel; }}
                    >
                      <div>
                        <div style={{ fontWeight: 600, fontSize: 14, display: "flex", alignItems: "center", gap: 8 }}>
                          {s.title}
                          <span style={{ fontSize: 10, color: T.text3, background: "#191c22", border: `1px solid ${T.lineSoft}`, padding: "1px 7px", borderRadius: 8 }}>进入 →</span>
                        </div>
                        <div style={{ fontSize: 12, color: T.text3, marginTop: 4 }}>起点：{s.time_origin} · 创建：{s.created_at} · 更新：{s.updated_at}</div>
                      </div>
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          removeStory(s.id, s.title);
                        }}
                        style={{ color: T.danger, background: "transparent", border: "none", cursor: "pointer", fontSize: 12.5, padding: "4px 10px" }}
                      >
                        删除
                      </button>
                    </div>
                  ))}
                </div>
              </div>
            )}


        {view === "debug" && (
          <div style={{ border: "1px solid #2e333d", padding: 12, borderRadius: 8, marginBottom: 12 }}>
            <div style={{ display: "flex", gap: 8, alignItems: "center", marginBottom: 8 }}>
              <h3 style={{ margin: 0 }}>🔍 请求快照（调试）</h3>
              <button onClick={loadDebug}>刷新</button>
              <button onClick={clearDebug}>清空</button>
            </div>
            <p style={{ fontSize: 12, color: "#a3a9b8", marginTop: 0 }}>最近 50 条发给 AI 的完整请求，点击一条查看详情</p>
            <div style={{ display: "flex", flexDirection: "column", gap: 4, maxHeight: 260, overflowY: "auto" }}>
              {debugList.length === 0 && <div style={{ fontSize: 12, color: "#6f7686" }}>暂无快照（发一条剧情后再刷新）</div>}
              {debugList.map((d) => (
                <button key={d.id} onClick={() => setSelectedDebug(selectedDebug?.id === d.id ? null : d)}
                  style={{ textAlign: "left", padding: "6px 10px", fontSize: 12, border: "1px solid #2e333d", borderRadius: 4, background: selectedDebug?.id === d.id ? "#3d4a66" : "#22262f", cursor: "pointer" }}>
                  #{d.id} [{d.status === "ok" ? "✅" : "❌"}] {d.created_at} · N={d.n_used} · ~{d.estimated_tokens} tok
                  <br />
                  <span style={{ color: "#a3a9b8" }}>{d.user_text.slice(0, 40)}</span>
                </button>
              ))}
            </div>
            {selectedDebug && (
              <div style={{ marginTop: 10, border: "1px solid #2e333d", borderRadius: 6, padding: 10, fontSize: 12, background: "#22262f", maxHeight: 420, overflowY: "auto" }}>
                <div><b>模型：</b>{selectedDebug.model || "-"}</div>
                <div><b>状态：</b>{selectedDebug.status} {selectedDebug.error ? "| " + selectedDebug.error : ""}</div>
                <div><b>N 轮：</b>{selectedDebug.n_used} | <b>估算 token：</b>{selectedDebug.estimated_tokens}</div>
                <hr style={{ margin: "8px 0" }} />
                <div><b>📋 系统提示词：</b></div>
                <pre style={{ whiteSpace: "pre-wrap", background: "#1a1d24", padding: 6, borderRadius: 4, fontSize: 11 }}>{selectedDebug.system_prompt}</pre>
                {selectedDebug.tail_block && (
                  <>
                    <div><b>🔄 上下文更新（尾部块）：</b><span style={{ color: "#8a91a1" }}>每轮现拼，并入本次用户消息（分隔线以上），不落库、不进下一轮历史</span></div>
                    <pre style={{ whiteSpace: "pre-wrap", background: "#3a2f1d", padding: 6, borderRadius: 4, fontSize: 11 }}>{selectedDebug.tail_block}</pre>
                  </>
                )}
                <div><b>💬 历史消息（JSON）：</b><span style={{ color: "#8a91a1" }}>已剔除上下文更新块</span></div>
                <pre style={{ whiteSpace: "pre-wrap", background: "#1a1d24", padding: 6, borderRadius: 4, fontSize: 11 }}>{(() => { try { const arr = JSON.parse(selectedDebug.history_json || "[]"); const cleaned = (Array.isArray(arr) ? arr : []).filter((m: unknown) => { const c = Array.isArray(m) ? String((m as unknown[])[1] ?? "") : String((m as { content?: unknown })?.content ?? ""); return !c.includes("【上下文更新】"); }); return JSON.stringify(cleaned, null, 2); } catch { return selectedDebug.history_json; } })()}</pre>
                <div><b>👤 用户输入：</b></div>
                <pre style={{ whiteSpace: "pre-wrap", background: "#3a3220", padding: 6, borderRadius: 4, fontSize: 11 }}>{selectedDebug.user_text}</pre>
                {selectedDebug.ai_reply && (
                  <>
                    <div><b>🤖 AI 回复：</b></div>
                    <pre style={{ whiteSpace: "pre-wrap", background: "#1d2f24", padding: 6, borderRadius: 4, fontSize: 11 }}>{selectedDebug.ai_reply}</pre>
                  </>
                )}
              </div>
            )}
          </div>
        )}

        {view === "chars" && (() => {
          const usageOf = (id: number) => charUsage.find((u) => u.id === id)?.dialogue_count ?? 0;
          const nodesOf = (id: number) => charUsage.find((u) => u.id === id)?.node_count ?? 0;
          const portraitsOf = (id: number) => portraits.filter((p) => p.character_id === id && p.story_id === currentStory?.id);
          const relCountOf = (id: number) => relations.filter((r) => r.from_character_id === id || r.to_character_id === id).length;
          const roleOptions = ["全部", "未定位", "男主", "女主", "对手", "配角", "背景", "其他", "主角"];
          const filteredChars = chars.filter((c) => {
            const q = charSearch.trim().toLowerCase();
            const hitQ = !q || [c.name, c.alias, c.role, c.faction, c.one_liner].some((s) => (s || "").toLowerCase().includes(q));
            const hitF = charFilter === "全部" || (c.role || "未定位") === charFilter;
            return hitQ && hitF;
          });
          const coverChars = new Set(portraits.filter((p) => p.character_id && p.story_id === currentStory?.id).map((p) => p.character_id)).size;
          const missingCover = chars.filter((c) => portraitsOf(c.id).length === 0).length;
          const labelStyle = { display: "flex", flexDirection: "column", gap: 4, flex: 1, fontSize: 11.5, color: T.text2 } as const;
          const inputStyle = { padding: "7px 10px", borderRadius: 7, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontSize: 12, fontFamily: "inherit", width: "100%", boxSizing: "border-box" } as const;
          const miniBtn = { height: 24, padding: "0 8px", borderRadius: 6, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, cursor: "pointer", fontSize: 10.5, fontFamily: "inherit" } as const;
          const accentBtn = { height: 32, padding: "0 14px", borderRadius: 8, border: "none", background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505", cursor: "pointer", fontSize: 12.5, fontWeight: 650, fontFamily: "inherit" } as const;
          const ghostBtn = { height: 32, padding: "0 12px", borderRadius: 8, border: `1px solid ${T.line}`, background: T.panel2, color: T.text2, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" } as const;
          const relNature = (r: Relation) => {
            const s = `${r.relation_type}${r.state}`;
            if (/敌|仇|对立|对抗|宿敌|杀/.test(s)) return { color: T.danger, dash: "", label: "敌对" };
            if (/信任|朋友|友|亲人|家人|兄妹|恋人|爱/.test(s)) return { color: T.ok, dash: "5 4", label: "可信任" };
            if (/待确认|未明|动摇|存疑|未知/.test(r.state)) return { color: "#5b8cff", dash: "4 4", label: "存疑" };
            if (r.strength >= 60) return { color: "#ff9a45", dash: "", label: "强关联" };
            return { color: "#6f7686", dash: "2 4", label: "弱关系" };
          };
          return (
            <div style={{ maxWidth: 1180, margin: "0 auto" }}>
              <div style={{ display: "flex", alignItems: "flex-start", gap: 12, marginBottom: 14 }}>
                <div>
                  <div style={{ fontSize: 15, fontWeight: 650 }}>👥 角色管理</div>
                  <div style={{ fontSize: 11.5, color: T.text3, marginTop: 4 }}>
                    {chars.length} 个角色 · 立绘覆盖 {coverChars} 角 · {missingCover > 0 ? <span style={{ color: T.danger }}>{missingCover} 个缺立绘</span> : "立绘齐全"} · {relations.length} 条关系
                  </div>
                </div>
                <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
                  <button onClick={() => setCharTab(charTab === "cards" ? "relations" : "cards")} style={ghostBtn}>{charTab === "cards" ? "🕸 关系图谱" : "🃏 角色卡片"}</button>
                  <button onClick={() => openCharForm(null)} style={accentBtn}>＋ 新建角色</button>
                </div>
              </div>
              <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap", marginBottom: 12 }}>
                <input value={charSearch} onChange={(e) => setCharSearch(e.target.value)} placeholder="搜索角色名、别名或身份" style={{ ...inputStyle, width: 240 }} />
                {roleOptions.map((r) => (
                  <button key={r} onClick={() => setCharFilter(r)} style={{ height: 28, padding: "0 11px", borderRadius: 14, border: `1px solid ${charFilter === r ? "rgba(255,122,24,.42)" : T.line}`, background: charFilter === r ? T.accentSoft : "#191c22", color: charFilter === r ? T.accent : T.text2, cursor: "pointer", fontSize: 11.5, fontFamily: "inherit" }}>{r}{r === "全部" ? ` ${chars.length}` : ""}</button>
                ))}
              </div>
              {charTab === "cards" && (
                <>
                  {charsLoading ? (
                    <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill,minmax(340px,1fr))", gap: 12 }}>
                      {[0, 1, 2].map((i) => (
                        <div key={i} style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, display: "grid", gridTemplateColumns: "104px 1fr", minHeight: 176, overflow: "hidden" }}>
                          <div style={{ background: "#22262f" }} />
                          <div style={{ padding: 12 }}>
                            <div style={{ height: 12, width: "42%", background: "#2a2f3a", borderRadius: 4, marginBottom: 10 }} />
                            <div style={{ height: 9, width: "78%", background: "#22262f", borderRadius: 4, marginBottom: 8 }} />
                            {[0, 1, 2, 3].map((j) => <div key={j} style={{ height: 6, background: "#1e222a", borderRadius: 3, marginBottom: 6 }} />)}
                          </div>
                        </div>
                      ))}
                    </div>
                  ) : chars.length === 0 ? (
                    <div style={{ border: `1px dashed ${T.line}`, borderRadius: 12, padding: "56px 20px", textAlign: "center", color: T.text3, background: T.panel }}>
                      <div style={{ fontSize: 30, marginBottom: 8 }}>👥</div>
                      <b style={{ color: T.text1, fontSize: 14 }}>还没有角色</b>
                      <p style={{ fontSize: 12, color: T.text3, margin: "8px 0 14px", lineHeight: 1.7 }}>先建一个视点人物，故事才有落点；也可以先写剧情，再逐步补角色档案。</p>
                      <button onClick={() => openCharForm(null)} style={accentBtn}>＋ 新建角色</button>
                    </div>
                  ) : (
                    <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill,minmax(340px,1fr))", gap: 12 }}>
                      {filteredChars.map((c) => {
                        const ps = portraitsOf(c.id);
                        const cover = ps.find((p) => p.path)?.path;
                        const roleColor = (c.role === "男主" || c.role === "女主" || c.role === "主角") ? T.accent : c.role === "对手" ? T.danger : c.role === "配角" ? "#5b8cff" : c.role === "背景" ? "#8f6bff" : T.text3;
                        return (
                          <div
                            key={c.id}
                            onClick={() => openCharForm(c)}
                            title="点击编辑该角色"
                            onMouseEnter={(e) => { e.currentTarget.style.borderColor = T.accent; }}
                            onMouseLeave={(e) => { e.currentTarget.style.borderColor = T.line; }}
                            style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, overflow: "hidden", display: "grid", gridTemplateColumns: "104px 1fr", minHeight: 178, cursor: "pointer", transition: "border-color .15s" }}
                          >
                            <div style={{ position: "relative", background: cover ? "#12141a" : "linear-gradient(160deg,#4a5f8a,#22293a)", overflow: "hidden" }}>
                              {cover ? <img src={convertFileSrc(cover)} alt="" style={{ width: "100%", height: "100%", objectFit: "cover" }} /> : <div style={{ height: "100%", display: "flex", alignItems: "center", justifyContent: "center", fontSize: 34, color: "rgba(255,255,255,.7)" }}>🧑</div>}
                              <span style={{ position: "absolute", top: 6, left: 6, fontSize: 9.5, padding: "2px 7px", borderRadius: 5, background: roleColor, color: "#fff", fontWeight: 600 }}>{c.role || "未定位"}</span>
                              <span style={{ position: "absolute", bottom: 0, left: 0, right: 0, fontSize: 9.5, padding: "3px 6px", background: "rgba(0,0,0,.58)", color: "#dfe3ec", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{c.current_status || "未出场"}</span>
                            </div>
                            <div style={{ padding: "10px 12px", display: "flex", flexDirection: "column", gap: 6, minWidth: 0 }}>
                              <div style={{ display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
                                <span style={{ fontWeight: 650, fontSize: 13.5 }}>{c.name}</span>
                                {c.alias && <span style={{ fontSize: 10, color: T.text3 }}>{c.alias}</span>}
                                {c.traits.split(/[,，、]/).filter(Boolean).slice(0, 2).map((t) => <span key={t} style={{ fontSize: 9.5, padding: "1px 6px", borderRadius: 8, background: T.accentSoft, color: T.accent }}>{t}</span>)}
                              </div>
                              <div style={{ fontSize: 10.5, color: T.text3, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{[c.one_liner || c.profile, [c.age, c.gender].filter(Boolean).join(" · "), c.faction].filter(Boolean).join(" · ") || "（暂无身份设定）"}</div>
                              <div style={{ display: "flex", flexDirection: "column", gap: 3 }}>
                                {([["体力", c.stat_stamina, "#f0554d,#ff9a45"], ["理智", c.stat_sanity, "#5b8cff,#8f6bff"], ["情绪", c.stat_emotion, "#f5b544,#f0554d"], ["戏份", c.stat_screen, "#ff7a18,#ffb066"]] as [string, number, string][]).map(([lb, v, grad]) => (
                                  <div key={lb} style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 9.5 }}>
                                    <span style={{ width: 22, color: T.text3 }}>{lb}</span>
                                    <span style={{ flex: 1, height: 5, borderRadius: 3, background: "#12141a", overflow: "hidden" }}><i style={{ display: "block", height: "100%", width: `${v}%`, background: `linear-gradient(90deg,${grad})` }} /></span>
                                    <span style={{ width: 20, textAlign: "right", color: T.text2, fontFamily: "monospace" }}>{v}</span>
                                  </div>
                                ))}
                              </div>
                              <div style={{ display: "flex", alignItems: "center", gap: 8, marginTop: "auto", paddingTop: 4, borderTop: `1px solid ${T.lineSoft}`, fontSize: 10, color: T.text3 }}>
                                <span>📖 {nodesOf(c.id)} 节点</span>
                                <span>💬 {usageOf(c.id)} 句</span>
                                <span>🖼 {ps.length} 张</span>
                                <span style={{ marginLeft: "auto" }} />
                                <button onClick={(e) => { e.stopPropagation(); openCharForm(c); }} style={miniBtn}>编辑</button>
                                <button onClick={(e) => { e.stopPropagation(); removeChar(c.id, c.name); }} style={{ ...miniBtn, color: T.danger }}>删除</button>
                              </div>
                            </div>
                          </div>
                        );
                      })}
                      <div onClick={() => openCharForm(null)} style={{ border: `1px dashed ${T.line}`, borderRadius: 12, minHeight: 178, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 6, color: T.text3, cursor: "pointer", background: "transparent" }}>
                        <span style={{ fontSize: 24 }}>＋</span>
                        <b style={{ fontSize: 12.5, color: T.text2 }}>新建角色</b>
                        <small style={{ fontSize: 10 }}>手写档案，或让 AI 生成</small>
                      </div>
                    </div>
                  )}
                </>
              )}
              {charTab === "relations" && (() => {
                const relRoot = (charGraphRootId != null ? chars.find((c) => c.id === charGraphRootId) : undefined)
                  ?? chars.find((c) => c.role === "男主" || c.role === "女主" || c.role === "主角")
                  ?? chars.slice().sort((a, b) => relCountOf(b.id) - relCountOf(a.id))[0]
                  ?? null;
                const centerRels = relRoot ? relations.filter((r) => r.from_character_id === relRoot.id || r.to_character_id === relRoot.id) : [];
                const connectedIds = new Set<number>();
                centerRels.forEach((r) => { connectedIds.add(r.from_character_id); connectedIds.add(r.to_character_id); });
                const others = relRoot ? chars.filter((c) => c.id !== relRoot.id && connectedIds.has(c.id)) : [];
                const pos = new Map<number, { x: number; y: number }>();
                if (relRoot) pos.set(relRoot.id, { x: 400, y: 190 });
                others.forEach((c, i) => {
                  const a = (2 * Math.PI * i) / Math.max(others.length, 1) - Math.PI / 2 + (others.length === 2 ? 0.55 : 0);
                  pos.set(c.id, { x: 400 + 250 * Math.cos(a), y: 190 + 128 * Math.sin(a) });
                });
                return (
                  <div style={{ display: "grid", gridTemplateColumns: "minmax(380px,1.25fr) minmax(310px,1fr)", gap: 14, alignItems: "start" }}>
                    <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: "#12141a", padding: 10 }}>
                      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "2px 4px 8px", fontSize: 11.5, color: T.text3, flexWrap: "wrap" }}>
                        <b style={{ color: T.text2 }}>关系网络</b>
                        <span>{chars.length} 角色 · {centerRels.length} 条直接关系</span>
                        <span style={{ marginLeft: "auto", display: "flex", gap: 8, flexWrap: "wrap" }}>
                          <span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}><i style={{ width: 14, borderTop: "2.4px solid #ff9a45", display: "block" }} />强关联</span>
                          <span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}><i style={{ width: 14, borderTop: `2px solid ${T.danger}`, display: "block" }} />敌对</span>
                          <span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}><i style={{ width: 14, borderTop: `1.6px dashed ${T.ok}`, display: "block" }} />可信任</span>
                          <span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}><i style={{ width: 14, borderTop: "1.6px dashed #5b8cff", display: "block" }} />存疑</span>
                        </span>
                      </div>
                      <div style={{ display: "flex", alignItems: "center", gap: 4, padding: "0 4px 8px", flexWrap: "wrap" }}>
                        <span style={{ fontSize: 10.5, color: T.text3, marginRight: 2 }}>中心角色</span>
                        <span onClick={() => setCharGraphRootId(null)} style={{ fontSize: 10.5, padding: "2px 8px", borderRadius: 10, cursor: "pointer", border: `1px solid ${charGraphRootId == null ? T.accent : T.line}`, background: charGraphRootId == null ? T.accentSoft : "#191c22", color: charGraphRootId == null ? T.accent : T.text2, fontWeight: charGraphRootId == null ? 600 : 400 }}>自动</span>
                        {chars.map((c) => {
                          const active = relRoot?.id === c.id;
                          return (
                            <span key={c.id} onClick={() => setCharGraphRootId(c.id)} style={{ fontSize: 10.5, padding: "2px 8px", borderRadius: 10, cursor: "pointer", border: `1px solid ${active ? T.accent : T.line}`, background: active ? T.accentSoft : "#191c22", color: active ? T.accent : T.text2, fontWeight: active ? 600 : 400 }}>{c.name}</span>
                          );
                        })}
                      </div>
                      {chars.length === 0 ? (
                        <div style={{ padding: "70px 20px", textAlign: "center", color: T.text3, fontSize: 12.5 }}>还没有角色，先切到「角色卡片」新建角色。</div>
                      ) : centerRels.length === 0 ? (
                        <div style={{ padding: "70px 20px", textAlign: "center", color: T.text3, fontSize: 12.5 }}>「{relRoot?.name ?? "该角色"}」暂无直接关系，可在右侧新增。</div>
                      ) : (
                        <svg viewBox="0 0 800 380" style={{ width: "100%", height: 380, display: "block" }}>
                          <defs>
                            <marker id="relArrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto" markerUnits="strokeWidth"><path d="M0,0 L8,4 L0,8 Z" fill="#ff9a45" /></marker>
                          </defs>
                          {centerRels.map((r) => {
                            const p1 = pos.get(r.from_character_id);
                            const p2 = pos.get(r.to_character_id);
                            if (!p1 || !p2) return null;
                            const nature = relNature(r);
                            const dx = p2.x - p1.x, dy = p2.y - p1.y;
                            const len = Math.sqrt(dx * dx + dy * dy) || 1;
                            const sx = p1.x + (dx / len) * (relRoot && r.from_character_id === relRoot.id ? 26 : 20);
                            const sy = p1.y + (dy / len) * (relRoot && r.from_character_id === relRoot.id ? 26 : 20);
                            const ex = p2.x - (dx / len) * (relRoot && r.to_character_id === relRoot.id ? 26 : 20);
                            const ey = p2.y - (dy / len) * (relRoot && r.to_character_id === relRoot.id ? 26 : 20);
                            return (
                              <g key={r.id}>
                                <line x1={sx} y1={sy} x2={ex} y2={ey} stroke={nature.color} strokeWidth={nature.label === "强关联" ? 2.4 : 1.7} strokeDasharray={nature.dash || undefined} markerEnd="url(#relArrow)" opacity={0.9} />
                                <text x={(sx + ex) / 2} y={(sy + ey) / 2 - 5} textAnchor="middle" fontSize="10" fill="#fff" fontWeight={600} paintOrder="stroke" stroke="#12141a" strokeWidth={3}>{(r.relation_type || "陌生人")}{r.strength ? ` ${r.strength}` : ""}</text>
                              </g>
                            );
                          })}
                          {chars.map((c) => {
                            const p = pos.get(c.id);
                            if (!p) return null;
                            const isRoot = relRoot?.id === c.id;
                            const emo = emotionInfo(c.current_status || "");
                            const dx = p.x - 400, dy = p.y - 190;
                            const dlen = Math.sqrt(dx * dx + dy * dy) || 1;
                            const r = isRoot ? 25 : 18;
                            const roleOutX = (dx / dlen) * (r + 12);
                            const roleOutY = (dy / dlen) * (r + 12);
                            const roleAnchor = dx > 10 ? "start" : dx < -10 ? "end" : "middle";
                            return (
                              <g key={c.id}>
                                <circle cx={p.x} cy={p.y} r={r} fill={isRoot ? T.accent : T.panel3} stroke={isRoot ? T.accent : (emo.label ? emo.color : T.line)} strokeWidth={isRoot ? 2 : 1.6} />
                                <text x={p.x} y={isRoot && c.role ? p.y - 2 : p.y + 3.5} textAnchor="middle" fontSize={isRoot ? 11.5 : 11} fill={isRoot ? "#231505" : T.text1} fontWeight={isRoot ? 700 : 600}>{c.name}</text>
                                {c.role && (isRoot ? (
                                  <text x={p.x} y={p.y + 10} textAnchor="middle" fontSize="8.5" fill="#231505" fontWeight={600}>{c.role}</text>
                                ) : (
                                  <text x={p.x + roleOutX} y={p.y + roleOutY} textAnchor={roleAnchor} fontSize="9" fill={T.text3} paintOrder="stroke" stroke="#12141a" strokeWidth={3}>{c.role}</text>
                                ))}
                              </g>
                            );
                          })}
                        </svg>
                      )}
                    </div>
                    <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
                      {chars.length >= 2 && (
                        <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, padding: 12 }}>
                          <div style={{ fontSize: 12.5, fontWeight: 650, marginBottom: 8 }}>{relEditingId ? "编辑关系" : "新增关系"}</div>
                          <div style={{ display: "grid", gridTemplateColumns: "1fr 18px 1fr", gap: 6, alignItems: "center" }}>
                            <select value={rFrom} onChange={(e) => setRFrom(Number(e.target.value))} style={{ ...inputStyle, padding: "6px 8px" }}>
                              <option value={0}>— 从 —</option>
                              {chars.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
                            </select>
                            <span style={{ textAlign: "center", color: T.text3 }}>{rDir === "双向" ? "⟷" : "→"}</span>
                            <select value={rTo} onChange={(e) => setRTo(Number(e.target.value))} style={{ ...inputStyle, padding: "6px 8px" }}>
                              <option value={0}>— 到 —</option>
                              {chars.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
                            </select>
                          </div>
                          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 6, marginTop: 6 }}>
                            <input value={rType} onChange={(e) => setRType(e.target.value)} placeholder="关系类型（父女 / 兄妹 / 敌对 / 同学）" style={{ ...inputStyle, padding: "6px 8px" }} />
                            <select value={rDir} onChange={(e) => setRDir(e.target.value as "单向" | "双向")} style={{ ...inputStyle, padding: "6px 8px" }}>
                              <option value="单向">单向</option>
                              <option value="双向">双向</option>
                            </select>
                          </div>
                          <div style={{ display: "grid", gridTemplateColumns: "1fr 90px", gap: 8, alignItems: "center", marginTop: 6 }}>
                            <input type="range" min={0} max={100} value={rStrength} onChange={(e) => setRStrength(Number(e.target.value))} />
                            <span style={{ fontSize: 11, color: T.text2, textAlign: "right" }}>强度 {rStrength}</span>
                          </div>
                          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 6, marginTop: 6 }}>
                            <select value={rState} onChange={(e) => setRState(e.target.value)} title={REL_STATE_HINT[rState] || ""} style={{ ...inputStyle, padding: "6px 8px" }}>
                              {REL_STATES.map((s) => <option key={s} value={s}>状态：{s}</option>)}
                            </select>
                            <input value={rEvidence} onChange={(e) => setREvidence(e.target.value)} placeholder="出处 / 证据" style={{ ...inputStyle, padding: "6px 8px" }} />
                          </div>
                          <div style={{ fontSize: 10.5, color: T.text3, marginTop: 4 }}>{REL_STATE_HINT[rState] || ""}</div>
                          <textarea value={rDescription} onChange={(e) => setRDescription(e.target.value)} rows={2} placeholder="关系描述（会注入 AI 的关系上下文）" style={{ ...inputStyle, marginTop: 6, resize: "vertical", lineHeight: 1.6 }} />
                          <div style={{ display: "flex", gap: 8, marginTop: 8 }}>
                            <button onClick={saveRelation} style={accentBtn}>{relEditingId ? "保存修改" : "＋ 添加关系"}</button>
                            {relEditingId && <button onClick={resetRelationForm} style={ghostBtn}>取消编辑</button>}
                          </div>
                        </div>
                      )}
                      <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, overflow: "hidden" }}>
                        <div style={{ padding: "10px 12px", background: T.panel2, borderBottom: `1px solid ${T.lineSoft}`, fontSize: 11.5, color: T.text2 }}>关系条目 · {relations.length} 条</div>
                        <div style={{ maxHeight: 420, overflowY: "auto" }}>
                          {relations.length === 0 && <div style={{ padding: "26px 14px", textAlign: "center", color: T.text3, fontSize: 12 }}>还没有关系。</div>}
                          {relations.map((r) => (
                            <div key={r.id} style={{ padding: "10px 12px", borderBottom: `1px solid ${T.lineSoft}`, display: "flex", flexDirection: "column", gap: 6 }}>
                              <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 12.5, fontWeight: 600 }}>
                                <span>{charNameOf(r.from_character_id)}</span>
                                <span style={{ color: T.text3 }}>{r.direction === "双向" ? "⟷" : "→"}</span>
                                <span>{charNameOf(r.to_character_id)}</span>
                                <span style={{ marginLeft: "auto", fontSize: 9.5, padding: "1px 7px", borderRadius: 8, border: `1px solid ${T.line}`, color: T.text3 }}>{r.relation_type || "陌生人"} · {r.strength} · {normRelState(r.state)}</span>
                              </div>
                              {r.direction === "双向" && r.split_matched && r.split_records.length > 0 && (
                                <button onClick={() => setRelExpandId(relExpandId === r.id ? null : r.id)} style={{ alignSelf: "flex-start", height: 22, padding: "0 8px", borderRadius: 6, border: `1px solid ${T.line}`, background: "#191c22", color: T.text3, cursor: "pointer", fontSize: 10, fontFamily: "inherit" }}>
                                  {relExpandId === r.id ? "收起自动拆分" : `展开自动拆分 ${r.split_records.length} 条`}
                                </button>
                              )}
                              {relExpandId === r.id && r.split_records.map((s, i) => (
                                <div key={`${r.id}-split-${i}`} style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 11, color: T.text2, background: "#191c22", border: `1px solid ${T.lineSoft}`, borderRadius: 7, padding: "5px 8px" }}>
                                  <span style={{ fontSize: 9.5, padding: "1px 6px", borderRadius: 6, background: T.accentSoft, color: T.accent }}>单向</span>
                                  <span>{s.text}</span>
                                </div>
                              ))}
                              {r.description && <div style={{ fontSize: 11, color: T.text2, lineHeight: 1.65 }}>{r.description}</div>}
                              {r.evidence && <div style={{ fontSize: 10.5, color: T.text3 }}>出处：{r.evidence}</div>}
                              <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 10, color: T.text3 }}>
                                <span title={REL_STATE_HINT[normRelState(r.state)] || ""} style={{ padding: "1px 6px", borderRadius: 7, background: "#191c22", border: `1px solid ${T.lineSoft}` }}>{normRelState(r.state)}</span>
                                <span style={{ marginLeft: "auto" }} />
                                <button onClick={() => editRelation(r)} style={miniBtn}>编辑</button>
                                <button onClick={() => removeRelation(r.id)} style={{ ...miniBtn, color: T.danger }}>删除</button>
                              </div>
                            </div>
                          ))}
                        </div>
                      </div>
                    </div>
                  </div>
                );
              })()}
              {charFormOpen && (
                <div onClick={() => setCharFormOpen(false)} style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,.56)", zIndex: 60, display: "flex", alignItems: "flex-start", justifyContent: "center", padding: "40px 20px", overflowY: "auto" }}>
                  <div onClick={(e) => e.stopPropagation()} style={{ width: 880, maxWidth: "100%", background: T.panel, border: `1px solid ${T.line}`, borderRadius: 14, boxShadow: "0 24px 70px rgba(0,0,0,.55)", overflow: "hidden" }}>
                    <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "13px 16px", borderBottom: `1px solid ${T.lineSoft}` }}>
                      <b style={{ fontSize: 14 }}>{charEditingId ? "编辑角色" : "新建角色"}</b>
                      <span style={{ fontSize: 11.5, color: T.text3 }}>{charEditingId ? "保存后版本 +1" : "左侧档案 + 右侧立绘预览"}</span>
                      <button onClick={() => setCharFormOpen(false)} style={{ marginLeft: "auto", background: "none", border: "none", color: T.text3, cursor: "pointer", fontSize: 16, fontFamily: "inherit" }}>✕</button>
                    </div>
                    <div style={{ display: "grid", gridTemplateColumns: "1fr 220px", gap: 16, padding: 16 }}>
                      <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
                        <div style={{ display: "flex", gap: 10 }}>
                          <label style={labelStyle}>姓名 *<input value={charForm.name} onChange={(e) => setCharForm({ ...charForm, name: e.target.value })} style={inputStyle} /></label>
                          <label style={labelStyle}>别名 / 称呼<input value={charForm.alias} onChange={(e) => setCharForm({ ...charForm, alias: e.target.value })} style={inputStyle} /></label>
                          <label style={{ ...labelStyle, flex: "0 0 110px" }}>定位<select value={charForm.role} onChange={(e) => setCharForm({ ...charForm, role: e.target.value })} style={inputStyle}>{["未定位", "男主", "女主", "对手", "配角", "背景", "其他", "主角"].map((r) => <option key={r} value={r}>{r}</option>)}</select></label>
                        </div>
                        <div style={{ display: "flex", gap: 10 }}>
                          <label style={labelStyle}>阵营 / 归属势力<input value={charForm.faction} onChange={(e) => setCharForm({ ...charForm, faction: e.target.value })} style={inputStyle} /></label>
                          <label style={labelStyle}>年龄<input value={charForm.age} onChange={(e) => setCharForm({ ...charForm, age: e.target.value })} style={inputStyle} /></label>
                          <label style={labelStyle}><span>性别 <span style={{ color: T.danger }}>*</span></span><input value={charForm.gender} onChange={(e) => setCharForm({ ...charForm, gender: e.target.value })} style={inputStyle} /></label>
                        </div>
                        <label style={labelStyle}>首次出场<input value={charForm.first_appearance} onChange={(e) => setCharForm({ ...charForm, first_appearance: e.target.value })} placeholder="第 1 章 第 1 节" style={inputStyle} /></label>
                        <label style={labelStyle}>一句话身份<input value={charForm.one_liner} onChange={(e) => setCharForm({ ...charForm, one_liner: e.target.value })} style={inputStyle} /></label>
                        <label style={labelStyle}>人物小传<textarea value={charForm.profile} onChange={(e) => setCharForm({ ...charForm, profile: e.target.value })} rows={3} style={{ ...inputStyle, resize: "vertical", lineHeight: 1.65 }} /></label>
                        <label style={labelStyle}>说话方式约束（注入 AI 生成）<textarea value={charForm.speech_style} onChange={(e) => setCharForm({ ...charForm, speech_style: e.target.value })} rows={2} style={{ ...inputStyle, resize: "vertical", lineHeight: 1.65 }} /></label>
                        <label style={labelStyle}>性格标签（逗号分隔）<input value={charForm.traits} onChange={(e) => setCharForm({ ...charForm, traits: e.target.value })} placeholder="克制，偏执，责任感过重" style={inputStyle} /></label>
                        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10 }}>
                          {([["体力", "stat_stamina"], ["理智", "stat_sanity"], ["情绪", "stat_emotion"], ["戏份", "stat_screen"]] as [string, keyof CharacterForm][]).map(([lb, key]) => (
                            <label key={lb} style={{ ...labelStyle, flex: "none" }}>
                              <span>{lb} <b style={{ color: T.accent }}>{charForm[key] as number}</b></span>
                              <input type="range" min={0} max={100} value={charForm[key] as number} onChange={(e) => setCharForm((f) => ({ ...f, [key]: Number(e.target.value) } as CharacterForm))} />
                            </label>
                          ))}
                        </div>
                        {charError && (
                          <div style={{ border: `1px solid ${T.danger}55`, background: "rgba(240,85,77,.08)", borderRadius: 8, padding: "10px 12px", fontSize: 11.5, color: "#ff8c85", lineHeight: 1.7 }}>
                            ⚠ {charError}
                            <div style={{ display: "flex", gap: 8, marginTop: 8 }}>
                              <button onClick={saveCharForm} style={miniBtn}>重试保存</button>
                              <button onClick={() => setCharError("")} style={miniBtn}>手动补全</button>
                            </div>
                          </div>
                        )}
                      </div>
                      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                        <div style={{ border: `1px dashed ${T.line}`, borderRadius: 12, minHeight: 230, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", color: T.text3, background: "#191c22", overflow: "hidden" }}>
                          {charEditingId && portraitsOf(charEditingId)[0]?.path ? (
                            <img src={convertFileSrc(portraitsOf(charEditingId)[0].path as string)} alt="" style={{ width: "100%", height: 230, objectFit: "cover" }} />
                          ) : (
                            <>
                              <span style={{ fontSize: 34 }}>🖼</span>
                              <span style={{ fontSize: 11.5, marginTop: 6 }}>立绘预览</span>
                              <small style={{ fontSize: 9.5 }}>未绑定</small>
                            </>
                          )}
                        </div>
                        <button onClick={() => setMsg("🎨 AI 生成立绘仍后置（规划中）")} style={ghostBtn}>✨ AI 生成立绘</button>
                        <button onClick={() => { setCharFormOpen(false); setView("portrait"); }} style={ghostBtn}>📤 从素材库选择</button>
                        <button onClick={randomCharForm} style={ghostBtn}>🎲 随机灵感</button>
                        <button onClick={saveCharForm} style={{ ...accentBtn, width: "100%" }}>💾 保存角色</button>
                        <div style={{ fontSize: 10, color: T.text3, lineHeight: 1.7 }}>保存后写入角色档案；说话方式、身份与小传会注入剧情 AI 的角色上下文。</div>
                      </div>
                    </div>
                  </div>
                </div>
              )}
            </div>
          );
        })()}
        {view === "world" && (() => {
          const catOf = (c: string) => worlds.filter((w) => w.category === c);
          const metaOf = (w: WorldSetting) => { try { return JSON.parse(w.meta_json || "{}") as Record<string, string>; } catch { return {} as Record<string, string>; } };
          const locs = catOf("地理");
          const facs = catOf("势力");
          const facRels = catOf("势力关系");
          const terms = catOf("术语");
          const plain = worlds.filter((w) => !["地理", "势力", "势力关系", "术语"].includes(w.category));
          const hardCount = worlds.filter((w) => w.is_hard_rule === 1).length;
          const resetDraft = (category: string, meta: Record<string, string> = {}) => { setWEditId(null); setWDraft({ key: "", value: "", category, hard: false, meta }); };
          const saveDraft = async () => {
            if (!currentStory) return;
            if (!wDraft.key.trim() || !wDraft.value.trim()) { setMsg("❌ 请填写名称和内容"); return; }
            const metaJson = JSON.stringify(wDraft.meta || {});
            try {
              if (wEditId) {
                await invoke("update_world_setting", { storyId: String(currentStory.id), id: wEditId, key: wDraft.key.trim(), value: wDraft.value.trim(), isHardRule: wDraft.hard, category: wDraft.category, metaJson });
                setMsg("✅ 设定已更新");
              } else {
                await invoke("create_world_setting", { storyId: String(currentStory.id), key: wDraft.key.trim(), value: wDraft.value.trim(), isHardRule: wDraft.hard, category: wDraft.category, metaJson });
                setMsg("✅ 设定已添加");
              }
              setWEditId(null);
              setWDraft({ key: "", value: "", category: wDraft.category, hard: false, meta: {} });
              await loadWorlds(currentStory.id);
            } catch (e) { setMsg("❌ 保存失败: " + String(e)); }
          };
          const editDraft = (w: WorldSetting) => { setWEditId(w.id); setWDraft({ key: w.key, value: w.value, category: w.category, hard: w.is_hard_rule === 1, meta: metaOf(w) }); };
          const detect = async () => {
            if (!currentStory) return;
            setWorldLoading(true);
            try { setWorldConflicts(await invoke<WorldConflict[]>("detect_world_conflicts", { storyId: String(currentStory.id) })); }
            catch (e) { setMsg("❌ 冲突检测失败: " + String(e)); }
            finally { setWorldLoading(false); }
          };
          const dropEntry = async (id: number, label: string) => {
            if (!currentStory) return;
            if (!confirm(`删除设定「${label}」？`)) return;
            try { await invoke("delete_world_setting", { storyId: String(currentStory.id), id }); setMsg("🗑️ 已删除"); await loadWorlds(currentStory.id); await detect(); }
            catch (e) { setMsg("❌ 删除失败: " + String(e)); }
          };
          const saveFacRel = async () => {
            if (!currentStory || !facRelDraft.from || !facRelDraft.to) { setMsg("❌ 请选择两个势力"); return; }
            if (facRelDraft.from === facRelDraft.to) { setMsg("❌ 两个势力不能相同"); return; }
            const metaJson = JSON.stringify({ from: facRelDraft.from, to: facRelDraft.to });
            try {
              if (facRelEditId) {
                await invoke("update_world_setting", { storyId: String(currentStory.id), id: facRelEditId, key: `${facRelDraft.from} → ${facRelDraft.to}`, value: facRelDraft.relation, isHardRule: false, category: "势力关系", metaJson });
                setMsg("✅ 势力关系已更新");
              } else {
                await invoke("create_world_setting", { storyId: String(currentStory.id), key: `${facRelDraft.from} → ${facRelDraft.to}`, value: facRelDraft.relation, isHardRule: false, category: "势力关系", metaJson });
                setMsg("✅ 势力关系已添加");
              }
              setFacRelEditId(null);
              setFacRelDraft({ from: "", to: "", relation: "中立" });
              await loadWorlds(currentStory.id);
            } catch (e) { setMsg("❌ 保存失败: " + String(e)); }
          };
          const editFacRel = (w: WorldSetting) => { const m = metaOf(w); setFacRelEditId(w.id); setFacRelDraft({ from: m.from || "", to: m.to || "", relation: w.value || "中立" }); };
          const relOf = (a: string, b: string) => facRels.find((r) => { const m = metaOf(r); return m.from === a && m.to === b; });
          // 孤儿势力关系：两端势力已被删除，留在库里但矩阵里渲染不出来（防止"删不掉也看不见"）
          const facNames = new Set(facs.map((f) => f.key));
          const orphanRels = facRels.filter((r) => { const m = metaOf(r); return !facNames.has(m.from || "") || !facNames.has(m.to || ""); });
          const orphanCountOf = (facKey: string) => facRels.filter((r) => { const m = metaOf(r); return m.from === facKey || m.to === facKey; }).length;
          const removeFaction = async (f: WorldSetting) => {
            const n = orphanCountOf(f.key);
            if (n > 0 && !confirm(`「${f.key}」还有 ${n} 条势力关系，删除势力后这些关系会变成无效记录（可在矩阵下方查看并清理）。仍要删除？`)) return;
            await removeWorld(f.id, f.key);
          };
          const cleanOrphan = async (r: WorldSetting) => {
            const m = metaOf(r);
            const known = (facNames.has(m.from || "") ? m.from : "") || (facNames.has(m.to || "") ? m.to : "");
            const kept = known ? `（保留其中仍存在的一方：${known}）` : "（两端势力都已不存在）";
            if (!confirm(`删除这条无效的势力关系「${m.from} → ${m.to}」${kept}？`)) return;
            await removeWorld(r.id, r.key);
          };
          const inputS = { padding: "7px 10px", borderRadius: 7, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontSize: 12, fontFamily: "inherit", width: "100%", boxSizing: "border-box" } as const;
          const labelS = { display: "flex", flexDirection: "column", gap: 4, flex: 1, fontSize: 11.5, color: T.text2 } as const;
          const accentS = { height: 32, padding: "0 14px", borderRadius: 8, border: "none", background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505", cursor: "pointer", fontSize: 12.5, fontWeight: 650, fontFamily: "inherit" } as const;
          const ghostS = { height: 32, padding: "0 12px", borderRadius: 8, border: `1px solid ${T.line}`, background: T.panel2, color: T.text2, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" } as const;
          const miniS = { height: 24, padding: "0 8px", borderRadius: 6, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, cursor: "pointer", fontSize: 10.5, fontFamily: "inherit" } as const;
          const boxS = { border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, padding: 12 } as const;
          const emptyS = { border: `1px dashed ${T.line}`, borderRadius: 12, padding: "44px 20px", textAlign: "center", color: T.text3, fontSize: 12.5, background: T.panel } as const;
          const selLoc = locs.find((l) => l.id === worldSelId) ?? locs[0] ?? null;
          const FAC_COLORS: Record<string, string> = {
            green: "linear-gradient(135deg,#3f8f6d,#1e3d30)",
            blue: "linear-gradient(135deg,#4f8fb8,#22404f)",
            red: "linear-gradient(135deg,#c9584f,#5a2521)",
            purple: "linear-gradient(135deg,#7b5cff,#3a2b6b)",
            gold: "linear-gradient(135deg,#d0913c,#5a3d17)",
          };
          const colorKeyOf = (v?: string) => (v && FAC_COLORS[v] ? v : Object.keys(FAC_COLORS).find((k) => FAC_COLORS[k] === v) || "green");
          const colorBgOf = (v?: string) => (v && FAC_COLORS[v]) || (v && v.startsWith("linear-gradient") ? v : FAC_COLORS.green);
          return (
            <div style={{ maxWidth: 1180, margin: "0 auto" }}>
              <div style={{ display: "flex", alignItems: "flex-start", gap: 12, marginBottom: 14 }}>
                <div>
                  <div style={{ fontSize: 15, fontWeight: 650 }}>🌍 世界设定</div>
                  <div style={{ fontSize: 11.5, color: T.text3, marginTop: 4 }}>
                    {worlds.length} 条设定 · {new Set(worlds.map((w) => w.category)).size} 个分类 · 硬规则 {hardCount} 条 · {worldConflicts.length > 0 ? <span style={{ color: T.danger }}>{worldConflicts.length} 处冲突</span> : "冲突未检测"}
                  </div>
                </div>
                <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
                  <button onClick={detect} style={ghostS}>⚠ 冲突检测</button>
                  <button onClick={() => setMsg("✨ AI 补全设定后置（规划中）")} style={ghostS}>✨ AI 补全</button>
                  <button onClick={() => resetDraft(worldTab === "map" ? "地理" : worldTab === "factions" ? "势力" : worldTab === "terms" ? "术语" : "设定", worldTab === "map" ? { x: "50", y: "50" } : {})} style={accentS}>＋ 新建</button>
                </div>
              </div>
              <div style={{ display: "flex", gap: 4, padding: 3, border: `1px solid ${T.line}`, borderRadius: 10, background: T.panel2, width: "fit-content", marginBottom: 14, flexWrap: "wrap" }}>
                {([["entries", "📚 设定条目"], ["map", "🗺 地图与地点"], ["factions", "🏛 势力矩阵"], ["terms", "📖 术语词典"], ["conflicts", "⚠ 冲突检测"]] as const).map(([k, lb]) => (
                  <button key={k} onClick={() => setWorldTab(k)} style={{ border: "none", borderRadius: 7, padding: "7px 14px", fontSize: 12.5, cursor: "pointer", background: worldTab === k ? T.accentSoft : "transparent", color: worldTab === k ? T.accent : T.text2, fontWeight: worldTab === k ? 600 : 400, fontFamily: "inherit" }}>{lb}</button>
                ))}
              </div>
              {worldTab === "entries" && (
                <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
                  <div style={boxS}>
                    <div style={{ display: "flex", gap: 10, flexWrap: "wrap" }}>
                      <label style={labelS}>条目名<input value={wDraft.category === "设定" ? wDraft.key : ""} onChange={(e) => setWDraft({ ...wDraft, category: "设定", key: e.target.value })} placeholder="时代背景 / 魔法体系 / 物理法则" style={inputS} /></label>
                      <label style={labelS}>内容<input value={wDraft.category === "设定" ? wDraft.value : ""} onChange={(e) => setWDraft({ ...wDraft, category: "设定", value: e.target.value })} placeholder="例如：现代都市，无超自然" style={inputS} /></label>
                      <label style={{ ...labelS, flex: "0 0 150px" }}>硬规则<label style={{ display: "flex", alignItems: "center", gap: 6, height: 32, fontSize: 11.5 }}><input type="checkbox" checked={wDraft.category === "设定" && wDraft.hard} onChange={(e) => setWDraft({ ...wDraft, category: "设定", hard: e.target.checked })} />AI 不可违反</label></label>
                    </div>
                    <div style={{ display: "flex", gap: 8, marginTop: 10 }}>
                      <button onClick={saveDraft} style={accentS}>{wEditId && wDraft.category === "设定" ? "保存修改" : "＋ 添加条目"}</button>
                      {wEditId && wDraft.category === "设定" && <button onClick={() => resetDraft("设定")} style={ghostS}>取消编辑</button>}
                    </div>
                  </div>
                  {plain.length === 0 ? <div style={emptyS}>还没有设定条目。</div> : plain.map((w) => (
                    <div
                      key={w.id}
                      onClick={() => editDraft(w)}
                      title="点击编辑该设定条目"
                      onMouseEnter={(e) => { e.currentTarget.style.borderColor = T.accent; e.currentTarget.style.background = "#20242d"; }}
                      onMouseLeave={(e) => { e.currentTarget.style.borderColor = T.line; e.currentTarget.style.background = T.panel; }}
                      style={{ border: `1px solid ${T.line}`, borderRadius: 10, background: T.panel, padding: "10px 12px", display: "flex", alignItems: "center", gap: 10, cursor: "pointer", transition: "border-color .15s, background .15s" }}
                    >
                      <div style={{ minWidth: 0, flex: 1 }}>
                        <div style={{ fontWeight: 600, fontSize: 13 }}>{w.key}{w.is_hard_rule === 1 && <span style={{ marginLeft: 8, fontSize: 10, background: T.danger, color: "#fff", padding: "1px 6px", borderRadius: 4 }}>硬规则</span>}</div>
                        <div style={{ fontSize: 12, color: T.text3, marginTop: 4 }}>{w.value}</div>
                      </div>
                      <button onClick={(e) => { e.stopPropagation(); editDraft(w); }} style={miniS}>编辑</button>
                      <button onClick={(e) => { e.stopPropagation(); removeWorld(w.id, w.key); }} style={{ ...miniS, color: T.danger }}>删除</button>
                    </div>
                  ))}
                </div>
              )}
              {worldTab === "map" && (
                <div style={{ display: "grid", gridTemplateColumns: "minmax(360px,1.2fr) minmax(300px,1fr)", gap: 14, alignItems: "start" }}>
                  <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: "#12141a", padding: 10 }}>
                    <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "2px 4px 8px", fontSize: 11.5, color: T.text3 }}>
                      <b style={{ color: T.text2 }}>🗺 地点示意图</b><span>{locs.length} 个地点</span><span style={{ marginLeft: "auto" }}>点图钉 / 右侧条目联动</span>
                    </div>
                    {locs.length === 0 ? (
                      <div style={{ padding: "80px 20px", textAlign: "center", color: T.text3, fontSize: 12.5 }}>还没有地点，先在右侧添加。</div>
                    ) : (
                      <svg viewBox="0 0 600 360" style={{ width: "100%", height: 360, display: "block" }}>
                        <defs><pattern id="wgrid" width="30" height="30" patternUnits="userSpaceOnUse"><path d="M30 0H0V30" fill="none" stroke="#20242d" strokeWidth="1" /></pattern></defs>
                        <rect x="0" y="0" width="600" height="360" fill="url(#wgrid)" />
                        {locs.map((l) => {
                          const m = metaOf(l);
                          const x = 40 + (Number(m.x || 50) / 100) * 520;
                          const y = 30 + (Number(m.y || 50) / 100) * 200;
                          const on = selLoc?.id === l.id;
                          const cur = m.current === "1" || m.current === "true";
                          return (
                            <g key={l.id} onClick={() => setWorldSelId(l.id)} style={{ cursor: "pointer" }}>
                              {cur && <circle cx={x} cy={y} r="15" fill="none" stroke={T.accent} strokeWidth="2" opacity=".85" />}
                              <circle cx={x} cy={y} r={on ? 9 : 7} fill={on ? T.accent : "#5b8cff"} stroke="#12141a" strokeWidth="2" />
                              <text x={x} y={y - 14} textAnchor="middle" fontSize="11" fill={on ? T.accent : "#c8ccd6"} fontWeight={600} paintOrder="stroke" stroke="#12141a" strokeWidth={3}>{l.key}</text>
                            </g>
                          );
                        })}
                      </svg>
                    )}
                  </div>
                  <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
                    <div style={boxS}>
                      <div style={{ fontSize: 12.5, fontWeight: 650, marginBottom: 8 }}>{wEditId && wDraft.category === "地理" ? "编辑地点" : "新增地点"}</div>
                      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 8 }}>
                        <label style={labelS}>地点名<input value={wDraft.category === "地理" ? wDraft.key : ""} onChange={(e) => setWDraft({ ...wDraft, category: "地理", key: e.target.value })} style={inputS} /></label>
                        <label style={labelS}>归属势力<input value={wDraft.category === "地理" ? (wDraft.meta.owner || "") : ""} onChange={(e) => setWDraft({ ...wDraft, category: "地理", meta: { ...(wDraft.category === "地理" ? wDraft.meta : {}), owner: e.target.value } })} style={inputS} /></label>
                        <label style={labelS}>区域<input value={wDraft.category === "地理" ? (wDraft.meta.region || "") : ""} onChange={(e) => setWDraft({ ...wDraft, category: "地理", meta: { ...(wDraft.category === "地理" ? wDraft.meta : {}), region: e.target.value } })} style={inputS} /></label>
                        <label style={labelS}>当前场景<select value={wDraft.category === "地理" ? (wDraft.meta.current || "0") : "0"} onChange={(e) => setWDraft({ ...wDraft, category: "地理", meta: { ...(wDraft.category === "地理" ? wDraft.meta : {}), current: e.target.value } })} style={inputS}><option value="0">否</option><option value="1">是</option></select></label>
                        <label style={labelS}>地图 X %<input value={wDraft.category === "地理" ? (wDraft.meta.x || "50") : "50"} onChange={(e) => setWDraft({ ...wDraft, category: "地理", meta: { ...(wDraft.category === "地理" ? wDraft.meta : {}), x: e.target.value } })} style={inputS} /></label>
                        <label style={labelS}>地图 Y %<input value={wDraft.category === "地理" ? (wDraft.meta.y || "50") : "50"} onChange={(e) => setWDraft({ ...wDraft, category: "地理", meta: { ...(wDraft.category === "地理" ? wDraft.meta : {}), y: e.target.value } })} style={inputS} /></label>
                      </div>
                      <label style={{ ...labelS, marginTop: 8 }}>描述<input value={wDraft.category === "地理" ? wDraft.value : ""} onChange={(e) => setWDraft({ ...wDraft, category: "地理", value: e.target.value })} style={inputS} /></label>
                      <div style={{ display: "flex", gap: 8, marginTop: 10 }}>
                        <button onClick={saveDraft} style={accentS}>{wEditId && wDraft.category === "地理" ? "保存修改" : "＋ 添加地点"}</button>
                        <button onClick={() => resetDraft("地理", { x: "50", y: "50" })} style={ghostS}>清空 / 取消</button>
                      </div>
                    </div>
                    <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                      {locs.map((l) => {
                        const m = metaOf(l);
                        const on = selLoc?.id === l.id;
                        return (
                          <div key={l.id} onClick={() => setWorldSelId(l.id)} style={{ border: `1px solid ${on ? T.accent : T.line}`, borderRadius: 10, background: T.panel, padding: "9px 11px", cursor: "pointer" }}>
                            <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 12.5, fontWeight: 600 }}>
                              {l.key}
                              {m.current === "1" && <span style={{ fontSize: 9.5, padding: "1px 6px", borderRadius: 8, background: T.accentSoft, color: T.accent }}>当前场景</span>}
                              <span style={{ marginLeft: "auto" }} />
                              <button onClick={(e) => { e.stopPropagation(); editDraft(l); }} style={miniS}>编辑</button>
                              <button onClick={(e) => { e.stopPropagation(); removeWorld(l.id, l.key); }} style={{ ...miniS, color: T.danger }}>删除</button>
                            </div>
                            <div style={{ fontSize: 11, color: T.text3, marginTop: 4 }}>{[m.region, m.owner].filter(Boolean).join(" · ") || "（未设置区域/归属）"}</div>
                            <div style={{ fontSize: 11.5, color: T.text2, marginTop: 4 }}>{l.value || "（无描述）"}</div>
                          </div>
                        );
                      })}
                    </div>
                  </div>
                </div>
              )}
              {worldTab === "factions" && (
                <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
                  <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill,minmax(250px,1fr))", gap: 10 }}>
                    {facs.map((f) => {
                      const m = metaOf(f);
                      return (
                        <div
                          key={f.id}
                          onClick={() => { editDraft(f); }}
                          title="点击编辑该势力"
                          onMouseEnter={(e) => { e.currentTarget.style.borderColor = T.accent; e.currentTarget.style.background = "#20242d"; }}
                          onMouseLeave={(e) => { e.currentTarget.style.borderColor = T.line; e.currentTarget.style.background = T.panel; }}
                          style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, overflow: "hidden", cursor: "pointer", transition: "border-color .15s, background .15s" }}
                        >
                          <div style={{ padding: "8px 12px", background: colorBgOf(m.color), color: "#fff", fontWeight: 650, fontSize: 13 }}>{f.key}</div>
                          <div style={{ padding: "10px 12px", display: "flex", flexDirection: "column", gap: 6, fontSize: 11.5, color: T.text2 }}>
                            <div style={{ display: "flex", gap: 8, color: T.text3 }}><span style={{ flex: "0 0 64px" }}>立场</span><span style={{ color: T.text2 }}>{f.value || "—"}</span></div>
                            {([["控制范围", m.control_range], ["规模", m.scale], ["头目", m.leader], ["对主角态度", m.attitude]] as [string, string | undefined][]).filter(([, v]) => v).map(([k, v]) => (
                              <div key={k} style={{ display: "flex", gap: 8, color: T.text3 }}><span style={{ flex: "0 0 64px" }}>{k}</span><span style={{ color: T.text2 }}>{v}</span></div>
                            ))}
                            <div style={{ display: "flex", gap: 6, marginTop: 4 }}>
                              <button onClick={(e) => { e.stopPropagation(); editDraft(f); }} style={miniS}>编辑</button>
                              <button onClick={(e) => { e.stopPropagation(); removeFaction(f); }} style={{ ...miniS, color: T.danger }}>删除</button>
                            </div>
                          </div>
                        </div>
                      );
                    })}
                    {facs.length === 0 && <div style={{ ...emptyS, gridColumn: "1 / -1" }}>还没有势力。</div>}
                  </div>
                  <div style={boxS}>
                    <div style={{ fontSize: 12.5, fontWeight: 650, marginBottom: 8 }}>{wEditId && wDraft.category === "势力" ? "编辑势力" : "新增势力"}</div>
                    <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: 8 }}>
                      <label style={labelS}>势力名<input value={wDraft.category === "势力" ? wDraft.key : ""} onChange={(e) => setWDraft({ ...wDraft, category: "势力", key: e.target.value })} style={inputS} /></label>
                      <label style={labelS}>立场 / 描述<input value={wDraft.category === "势力" ? wDraft.value : ""} onChange={(e) => setWDraft({ ...wDraft, category: "势力", value: e.target.value })} style={inputS} /></label>
                      <label style={labelS}>主色<select value={wDraft.category === "势力" ? colorKeyOf(wDraft.meta.color) : "green"} onChange={(e) => setWDraft({ ...wDraft, category: "势力", meta: { ...(wDraft.category === "势力" ? wDraft.meta : {}), color: e.target.value } })} style={inputS}>{([["green", "绿色"], ["blue", "蓝色"], ["red", "红色"], ["purple", "紫色"], ["gold", "金色"]] as const).map(([k, lb]) => <option key={k} value={k}>{lb}</option>)}</select></label>
                      <label style={labelS}>控制范围<input value={wDraft.category === "势力" ? (wDraft.meta.control_range || "") : ""} onChange={(e) => setWDraft({ ...wDraft, category: "势力", meta: { ...(wDraft.category === "势力" ? wDraft.meta : {}), control_range: e.target.value } })} style={inputS} /></label>
                      <label style={labelS}>规模<input value={wDraft.category === "势力" ? (wDraft.meta.scale || "") : ""} onChange={(e) => setWDraft({ ...wDraft, category: "势力", meta: { ...(wDraft.category === "势力" ? wDraft.meta : {}), scale: e.target.value } })} style={inputS} /></label>
                      <label style={labelS}>头目<input value={wDraft.category === "势力" ? (wDraft.meta.leader || "") : ""} onChange={(e) => setWDraft({ ...wDraft, category: "势力", meta: { ...(wDraft.category === "势力" ? wDraft.meta : {}), leader: e.target.value } })} style={inputS} /></label>
                      <label style={labelS}>对主角态度<input value={wDraft.category === "势力" ? (wDraft.meta.attitude || "") : ""} onChange={(e) => setWDraft({ ...wDraft, category: "势力", meta: { ...(wDraft.category === "势力" ? wDraft.meta : {}), attitude: e.target.value } })} style={inputS} /></label>
                      <div style={{ gridColumn: "1 / -1", display: "flex", gap: 8 }}>
                        <button onClick={saveDraft} style={accentS}>{wEditId && wDraft.category === "势力" ? "保存修改" : "＋ 添加势力"}</button>
                        <button onClick={() => resetDraft("势力")} style={ghostS}>清空 / 取消</button>
                      </div>
                    </div>
                  </div>
                  <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, overflow: "hidden" }}>
                    <div style={{ padding: "10px 12px", background: T.panel2, borderBottom: `1px solid ${T.lineSoft}`, fontSize: 12 }}>势力关系矩阵 · {facRels.length} 条关系</div>
                    {facs.length < 2 ? (
                      <div style={{ padding: "30px 14px", textAlign: "center", color: T.text3, fontSize: 12 }}>至少 2 个势力才能形成矩阵。</div>
                    ) : (
                      <div style={{ overflowX: "auto", padding: 12 }}>
                        <table style={{ borderCollapse: "collapse", width: "100%", fontSize: 11 }}>
                          <thead><tr><th></th>{facs.map((f) => <th key={f.id} style={{ padding: 6, color: T.text3, fontWeight: 500 }}>{f.key}</th>)}</tr></thead>
                          <tbody>
                            {facs.map((a) => (
                              <tr key={a.id}>
                                <td style={{ padding: 6, color: T.text3, whiteSpace: "nowrap" }}>{a.key}</td>
                                {facs.map((b) => {
                                  if (a.id === b.id) return <td key={b.id} style={{ padding: 6, textAlign: "center", color: T.text3 }}>—</td>;
                                  const rel = relOf(a.key, b.key);
                                  const label = rel?.value || "";
                                  const color = label.includes("敌") ? T.danger : label.includes("同盟") ? T.ok : label.includes("紧张") ? T.warn : T.text3;
                                  return (
                                    <td key={b.id} style={{ padding: 4, textAlign: "center" }}>
                                      <span onClick={() => { if (rel) editFacRel(rel); else { setFacRelEditId(null); setFacRelDraft({ from: a.key, to: b.key, relation: "中立" }); } }} style={{ display: "inline-block", cursor: "pointer", fontSize: 10.5, padding: "2px 8px", borderRadius: 9, border: `1px solid ${label ? color + "66" : T.line}`, background: label ? color + "18" : "#191c22", color: label ? color : T.text3 }}>{label || "＋"}</span>
                                    </td>
                                  );
                                })}
                              </tr>
                            ))}
                          </tbody>
                        </table>
                      </div>
                    )}
                    <div style={{ display: "flex", gap: 8, alignItems: "flex-end", padding: "0 12px 12px", flexWrap: "wrap" }}>
                      <label style={labelS}>从<select value={facRelDraft.from} onChange={(e) => setFacRelDraft({ ...facRelDraft, from: e.target.value })} style={inputS}><option value="">—</option>{facs.map((f) => <option key={f.id} value={f.key}>{f.key}</option>)}</select></label>
                      <label style={labelS}>到<select value={facRelDraft.to} onChange={(e) => setFacRelDraft({ ...facRelDraft, to: e.target.value })} style={inputS}><option value="">—</option>{facs.map((f) => <option key={f.id} value={f.key}>{f.key}</option>)}</select></label>
                      <label style={labelS}>关系<select value={facRelDraft.relation} onChange={(e) => setFacRelDraft({ ...facRelDraft, relation: e.target.value })} style={inputS}>{["敌对", "紧张", "冷处理", "同盟", "中立"].map((x) => <option key={x} value={x}>{x}</option>)}</select></label>
                      <button onClick={saveFacRel} style={accentS}>{facRelEditId ? "保存关系" : "＋ 设置关系"}</button>
                      {facRelEditId && <button onClick={async () => { const rel = facRels.find((x) => x.id === facRelEditId); if (rel) await removeWorld(rel.id, rel.key); setFacRelEditId(null); setFacRelDraft({ from: "", to: "", relation: "中立" }); }} style={{ ...ghostS, color: T.danger }}>删除关系</button>}
                      {facRelEditId && <button onClick={() => { setFacRelEditId(null); setFacRelDraft({ from: "", to: "", relation: "中立" }); }} style={ghostS}>取消</button>}
                    </div>
                    {orphanRels.length > 0 && (
                      <div style={{ borderTop: `1px solid ${T.lineSoft}`, padding: "10px 12px", background: "rgba(240,85,77,.045)" }}>
                        <div style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 11.5, color: "#ff9d97", marginBottom: 8 }}>
                          <span>⚠ 无效势力关系 {orphanRels.length} 条（关联的势力已被删除，矩阵里显示不出来）</span>
                        </div>
                        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                          {orphanRels.map((r) => {
                            const m = metaOf(r);
                            const missing = [m.from, m.to].filter((x) => !facNames.has(x || "")).join("、");
                            return (
                              <div key={r.id} style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 11, background: T.panel2, border: `1px solid ${T.line}`, borderRadius: 8, padding: "6px 10px" }}>
                                <span style={{ color: T.text2 }}>{m.from} <span style={{ color: T.text3 }}>→</span> {m.to}</span>
                                <span style={{ color: T.text3 }}>· {r.value || "中立"}</span>
                                <span style={{ marginLeft: "auto", color: T.text3 }}>缺失：{missing || "—"}</span>
                                <button onClick={() => cleanOrphan(r)} style={{ ...miniS, color: T.danger }}>删除这条</button>
                              </div>
                            );
                          })}
                        </div>
                        <div style={{ fontSize: 10.5, color: T.text3, marginTop: 8 }}>清理建议：先重建同名势力，再到矩阵里重设关系；或直接删掉这些无效记录。</div>
                      </div>
                    )}
                  </div>
                </div>
              )}
              {worldTab === "terms" && (
                <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
                  <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
                    <input value={worldSearch} onChange={(e) => setWorldSearch(e.target.value)} placeholder="搜索术语 / 别名 / 释义" style={{ ...inputS, width: 260 }} />
                    <span style={{ fontSize: 11.5, color: T.text3 }}>共 {terms.length} 个词条</span>
                  </div>
                  <div style={boxS}>
                    <div style={{ fontSize: 12.5, fontWeight: 650, marginBottom: 8 }}>{wEditId && wDraft.category === "术语" ? "编辑词条" : "新增词条"}</div>
                    <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 8 }}>
                      <label style={labelS}>术语<input value={wDraft.category === "术语" ? wDraft.key : ""} onChange={(e) => setWDraft({ ...wDraft, category: "术语", key: e.target.value })} style={inputS} /></label>
                      <label style={labelS}>别名（逗号分隔）<input value={wDraft.category === "术语" ? (wDraft.meta.aliases || "") : ""} onChange={(e) => setWDraft({ ...wDraft, category: "术语", meta: { ...(wDraft.category === "术语" ? wDraft.meta : {}), aliases: e.target.value } })} style={inputS} /></label>
                    </div>
                    <label style={{ ...labelS, marginTop: 8 }}>释义<input value={wDraft.category === "术语" ? wDraft.value : ""} onChange={(e) => setWDraft({ ...wDraft, category: "术语", value: e.target.value })} style={inputS} /></label>
                    <label style={{ ...labelS, marginTop: 8 }}>出现章节<input value={wDraft.category === "术语" ? (wDraft.meta.chapters || "") : ""} onChange={(e) => setWDraft({ ...wDraft, category: "术语", meta: { ...(wDraft.category === "术语" ? wDraft.meta : {}), chapters: e.target.value } })} style={inputS} /></label>
                    <div style={{ display: "flex", gap: 8, marginTop: 10 }}>
                      <button onClick={saveDraft} style={accentS}>{wEditId && wDraft.category === "术语" ? "保存修改" : "＋ 添加词条"}</button>
                      <button onClick={() => resetDraft("术语")} style={ghostS}>清空 / 取消</button>
                    </div>
                  </div>
                  <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, overflow: "hidden" }}>
                    <div style={{ display: "grid", gridTemplateColumns: "160px 150px 1fr 130px 100px", gap: 10, padding: "9px 22px", background: T.panel2, fontSize: 10.5, color: T.text3 }}>
                      <span>术语</span><span>别名</span><span>释义</span><span>出现章节</span><span style={{ textAlign: "right" }}>操作</span>
                    </div>
                    {terms.filter((w) => { const m = metaOf(w); const q = worldSearch.trim().toLowerCase(); return !q || w.key.toLowerCase().includes(q) || (m.aliases || "").toLowerCase().includes(q) || w.value.toLowerCase().includes(q); }).map((w) => {
                      const m = metaOf(w);
                      return (
                        <div
                          key={w.id}
                          onClick={() => editDraft(w)}
                          title="点击编辑该词条"
                          onMouseEnter={(e) => { e.currentTarget.style.borderColor = T.accent; e.currentTarget.style.background = "#20242d"; }}
                          onMouseLeave={(e) => { e.currentTarget.style.borderColor = T.line; e.currentTarget.style.background = T.panel; }}
                          style={{ display: "grid", gridTemplateColumns: "160px 150px 1fr 130px 100px", gap: 10, padding: "9px 12px", margin: "6px 10px 0", border: `1px solid ${T.line}`, borderRadius: 10, fontSize: 11.5, alignItems: "center", cursor: "pointer", transition: "border-color .15s, background .15s" }}
                        >
                          <span style={{ color: T.text1, fontWeight: 600 }}>{w.key}</span>
                          <span style={{ color: T.text3 }}>{m.aliases || "—"}</span>
                          <span style={{ color: T.text2 }}>{w.value}</span>
                          <span style={{ color: T.text3 }}>{m.chapters || "—"}</span>
                          <span style={{ textAlign: "right" }}><button onClick={(e) => { e.stopPropagation(); editDraft(w); }} style={miniS}>编辑</button> <button onClick={(e) => { e.stopPropagation(); removeWorld(w.id, w.key); }} style={{ ...miniS, color: T.danger }}>删除</button></span>
                        </div>
                      );
                    })}
                    {terms.length === 0 && <div style={{ padding: "26px 14px", textAlign: "center", color: T.text3, fontSize: 12 }}>还没有术语词条。</div>}
                  </div>
                </div>
              )}
              {worldTab === "conflicts" && (
                <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
                  <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                    <button onClick={detect} style={accentS}>⚠ 开始冲突检测</button>
                    <span style={{ fontSize: 11.5, color: T.text3 }}>当前仅检测「地图与地点」坐标重复（同一 x,y 被两个地点使用）；设定条目冲突检测暂未覆盖，标记为后期修正</span>
                  </div>
                  {worldLoading ? (
                    <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
                      {[0, 1].map((i) => <div key={i} style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, padding: 14 }}><div style={{ height: 12, width: "38%", background: "#2a2f3a", borderRadius: 4, marginBottom: 10 }} /><div style={{ height: 60, background: "#1e222a", borderRadius: 8 }} /></div>)}
                    </div>
                  ) : worldConflicts.length === 0 ? (
                    <div style={emptyS}>✅ 当前没有检测到设定冲突。点上方按钮重新检测。</div>
                  ) : (
                    worldConflicts.map((c, i) => (
                      <div key={i} style={{ border: `1px solid ${c.level === "严重" ? T.danger + "66" : T.line}`, borderRadius: 12, background: T.panel, overflow: "hidden" }}>
                        <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "10px 12px", background: T.panel2, fontSize: 12.5 }}>
                          <span>⚠</span><b>{c.title}</b>
                          <span style={{ marginLeft: "auto", fontSize: 10, padding: "1px 8px", borderRadius: 8, color: c.level === "严重" ? T.danger : T.warn, border: `1px solid ${c.level === "严重" ? T.danger + "66" : T.warn + "66"}` }}>{c.level}</span>
                        </div>
                        <div style={{ display: "grid", gridTemplateColumns: "1fr 1px 1fr", gap: 10, padding: 12 }}>
                          <div><div style={{ fontSize: 10.5, color: T.text3, marginBottom: 4 }}>{c.left_label}</div><div style={{ fontSize: 11.5, color: T.text2, lineHeight: 1.7 }}>{c.left_text}</div><div style={{ fontSize: 10, color: T.text3, marginTop: 6 }}>{c.left_source}</div></div>
                          <div style={{ background: T.lineSoft }} />
                          <div><div style={{ fontSize: 10.5, color: T.text3, marginBottom: 4 }}>{c.right_label}</div><div style={{ fontSize: 11.5, color: T.text2, lineHeight: 1.7 }}>{c.right_text}</div><div style={{ fontSize: 10, color: T.text3, marginTop: 6 }}>{c.right_source}</div></div>
                        </div>
                        <div style={{ display: "flex", gap: 8, padding: "0 12px 12px" }}>
                          <button onClick={() => dropEntry(c.right_id, c.title)} style={ghostS}>保留左侧</button>
                          <button onClick={() => dropEntry(c.left_id, c.title)} style={ghostS}>保留右侧</button>
                          <button onClick={() => setMsg("已暂时忽略该冲突")} style={ghostS}>暂时忽略</button>
                        </div>
                      </div>
                    ))
                  )}
                </div>
              )}
            </div>
          );
        })()}
        {view === "proposals" && (
          <div>
            <div style={{ display: "flex", gap: 8, alignItems: "center", marginBottom: 8, flexWrap: "wrap" }}>
              <h3 style={{ margin: 0 }}>📋 设定演进提案</h3>
              <button onClick={analyzeCurrent}>分析当前节点</button>
              <button onClick={loadProposals}>刷新</button>
              <span style={{ fontSize: 12, color: "#8a91a1" }}>强度变更自动应用；关系类型/方向变更需人工确认</span>
            </div>
            {proposals.length === 0 && <p style={{ color: "#6f7686" }}>暂无提案。先写剧情生成回复，再点"分析当前节点"。</p>}
            {Array.from(new Set(proposals.map((p) => p.group_id))).map((gid) => {
              const group = proposals.filter((p) => p.group_id === gid);
              const st = group[0].status;
              const badge =
                st === "pending" ? { text: "⏳ 待确认", color: "#ffd27d", bg: "rgba(245,181,68,.12)" }
                : st === "applied" ? { text: "✅ 已应用", color: "#7ddf9c", bg: "rgba(55,201,120,.12)" }
                : { text: "⛔ 已拒绝", color: "#ff8c85", bg: "rgba(240,85,77,.12)" };
              return (
                <div key={gid} style={{ border: "1px solid #2e333d", padding: 12, borderRadius: 8, marginBottom: 12 }}>
                  <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 8 }}>
                    <div style={{ fontWeight: 600 }}>提案组 #{gid} · {proposalRelationLabel(group[0])}</div>
                    <span style={{ fontSize: 12, padding: "2px 8px", borderRadius: 10, color: badge.color, background: badge.bg }}>{badge.text}</span>
                  </div>
                  <div style={{ fontSize: 13, color: "#dfe3ec", marginBottom: 6 }}>
                    {group.map((p) => (
                      <div key={p.id} style={{ marginBottom: 3 }}>
                        {p.field === "strength" ? "强度" : p.field === "direction" ? "方向" : "关系类型"}：
                        <b>{p.old_value}</b> → <b style={{ color: "#1890ff" }}>{p.new_value}</b>
                        <span style={{ color: "#6f7686", marginLeft: 6 }}>（置信度 {p.confidence.toFixed(2)}）</span>
                      </div>
                    ))}
                  </div>
                  <div style={{ fontSize: 12, color: "#a3a9b8", marginBottom: 6 }}>原因：{group[0].reason}</div>
                  <div style={{ fontSize: 12, color: "#8a91a1", marginBottom: 6 }}>证据：{group[0].evidence}</div>
                  {group[0].note && <div style={{ fontSize: 12, color: "#ff8c85" }}>备注：{group[0].note}</div>}
                  {st === "pending" && (
                    <div style={{ display: "flex", gap: 8, marginTop: 8 }}>
                      <button onClick={() => confirmGroup(gid)} style={{ background: "#52C41A", color: "#fff", border: "none", padding: "6px 14px", borderRadius: 6, cursor: "pointer" }}>接受并应用</button>
                      <button onClick={() => rejectGroup(gid)} style={{ background: "#EA6668", color: "#fff", border: "none", padding: "6px 14px", borderRadius: 6, cursor: "pointer" }}>拒绝</button>
                    </div>
                  )}
                </div>
              );
            })}

            <hr style={{ margin: "20px 0" }} />

            <h3 style={{ margin: 0, marginBottom: 8 }}>🗂 设定快照（可回滚）</h3>
            {snapshots.length === 0 ? (
              <p style={{ color: "#6f7686", fontSize: 13 }}>暂无快照。每次应用提案后自动生成新快照。</p>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 6, marginBottom: 12 }}>
                {snapshots.map((s) => (
                  <div key={s.id} style={{ border: "1px solid #2e333d", padding: "8px 12px", borderRadius: 8, display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                    <div style={{ fontSize: 13 }}>
                      V{s.version} · {s.created_at} {s.parent_version != null ? `（父 V${s.parent_version}）` : "（初始）"}
                    </div>
                    <button onClick={() => rollbackTo(s.version)} style={{ fontSize: 12, color: "#ff8c85", border: "1px solid rgba(240,85,77,.4)", background: "#22262f", padding: "3px 10px", borderRadius: 6, cursor: "pointer" }}>回滚到此版本</button>
                  </div>
                ))}
              </div>
            )}

            <h3 style={{ margin: 0, marginBottom: 8 }}>📜 审计日志</h3>
            {auditLogs.length === 0 ? (
              <p style={{ color: "#6f7686", fontSize: 13 }}>暂无审计记录。</p>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 4, marginBottom: 12 }}>
                {auditLogs.map((a) => (
                  <div key={a.id} style={{ fontSize: 12, color: "#a3a9b8" }}>
                    [{a.created_at}] {a.action} · {a.target || "-"} · {a.operator}
                  </div>
                ))}
              </div>
            )}

            <h3 style={{ margin: 0, marginBottom: 8 }}>📌 事件日志（分析器结果）</h3>
            {eventLogs.length === 0 ? (
              <p style={{ color: "#6f7686", fontSize: 13 }}>暂无事件日志。</p>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                {eventLogs.map((e) => (
                  <div key={e.id} style={{ fontSize: 12, color: "#a3a9b8" }}>
                    #{e.id} node{e.node_id ?? "-"} · {e.event_type} · 置信度 {e.confidence.toFixed(2)} · {e.source}
                  </div>
                ))}
              </div>
            )}
          </div>
        )}

                {view === "cache" && (
          <div style={{ padding: 16 }}>
            <h3 style={{ margin: 0, marginBottom: 8 }}>💾 缓存管理</h3>
            <p style={{ fontSize: 13, color: "#a3a9b8", margin: 0 }}>
              当前占用：<b>{(cacheUsageBytes / 1073741824).toFixed(2)} GB</b>（{cacheList.length} 条记录）
            </p>
            <div style={{ display: "flex", alignItems: "center", gap: 8, margin: "10px 0" }}>
              <span style={{ fontSize: 13 }}>上限（GB）</span>
              <input
                type="number"
                value={cacheLimit}
                onChange={(e) => setCacheLimit(Number(e.target.value))}
                style={{ width: 80, padding: "4px 8px" }}
              />
              <button onClick={saveCacheLimit} style={{ fontSize: 13, padding: "4px 12px", cursor: "pointer" }}>保存上限</button>
              <button onClick={autoClean} style={{ fontSize: 13, padding: "4px 12px", cursor: "pointer" }}>自动清理</button>
              <button onClick={clearCacheNow} style={{ fontSize: 13, padding: "4px 12px", cursor: "pointer", color: "#ff8c85" }}>清空</button>
            </div>
            {cacheList.length === 0 ? (
              <p style={{ color: "#6f7686", fontSize: 13 }}>暂无缓存记录。</p>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                {cacheList.map((x, i) => (
                  <div key={i} style={{ fontSize: 12, color: "#a3a9b8", display: "flex", justifyContent: "space-between", borderBottom: "1px solid #262a33", padding: "4px 0" }}>
                    <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", maxWidth: "60%" }}>{x.path}</span>
                    <span>{x.type_} · {(x.size / 1024).toFixed(1)} KB · {x.last_access}</span>
                  </div>
                ))}
              </div>
            )}
          </div>
        )}

        {view === "timeline" && (
          <div>
            <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 10 }}>
              <h3 style={{ margin: 0, fontSize: 15 }}>📅 时间线</h3>
              <span style={{ fontSize: 12, color: "#8a91a1" }}>点击节点可进入对应日期剧情</span>
              <div style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
                <button onClick={() => setTimelineView("list")} style={{ fontSize: 12, padding: "4px 12px", cursor: "pointer", borderRadius: 6, border: `1px solid ${T.line}`, background: timelineView === "list" ? T.accentSoft : "#191c22", color: timelineView === "list" ? T.accent : T.text2, fontFamily: "inherit" }}>列表</button>
                <button onClick={() => setTimelineView("cal")} style={{ fontSize: 12, padding: "4px 12px", cursor: "pointer", borderRadius: 6, border: `1px solid ${T.line}`, background: timelineView === "cal" ? T.accentSoft : "#191c22", color: timelineView === "cal" ? T.accent : T.text2, fontFamily: "inherit" }}>日历</button>
              </div>
            </div>
            {timelineNodes.length === 0 && <p style={{ color: "#6f7686" }}>暂无节点，先在剧情页创建。</p>}
            {timelineView === "list" && (
              <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                {timelineNodes.map((n) => {
                  const nd = nodeDateDisplay(n);
                  return (
                  <div
                    key={n.id}
                    onClick={() => { switchNode(n.id); setView("story"); }}
                    title="点击进入该日期剧情"
                    style={{
                      border: `1px solid ${T.line}`,
                      borderRadius: 10,
                      padding: "13px 16px",
                      display: "flex",
                      justifyContent: "space-between",
                      alignItems: "center",
                      cursor: "pointer",
                      background: T.panel,
                      transition: "border-color .15s, background .15s",
                    }}
                    onMouseEnter={(e) => { e.currentTarget.style.borderColor = T.accent; e.currentTarget.style.background = "#20242d"; }}
                    onMouseLeave={(e) => { e.currentTarget.style.borderColor = T.line; e.currentTarget.style.background = T.panel; }}
                  >
                    <div>
                      <div style={{ fontWeight: 600, fontSize: 14, display: "flex", alignItems: "center", gap: 8 }}>
                        {n.node_type === "interlude" ? "🎭 " : ""}{n.date_label}
                        {nd && <span style={{ fontSize: 11, color: T.text3, fontWeight: 400 }}>{nd}</span>}
                        <span style={{ fontSize: 10, color: T.text3, background: "#191c22", border: `1px solid ${T.lineSoft}`, padding: "1px 7px", borderRadius: 8 }}>进入 →</span>
                      </div>
                      <div style={{ fontSize: 11, color: "#8a91a1", marginTop: 4 }}>{n.node_type === "interlude" ? "插曲 / 回忆" : "常规节点"}</div>
                      {n.tags ? (
                        <div style={{ display: "flex", gap: 4, marginTop: 4, flexWrap: "wrap" }}>
                          {n.tags.split(",").filter(Boolean).map((t) => (
                            <span key={t} style={{ fontSize: 10, color: T.accent, background: T.accentSoft, borderRadius: 6, padding: "1px 7px" }}>#{t}</span>
                          ))}
                        </div>
                      ) : null}
                    </div>
                    <button
                      onClick={(e) => { e.stopPropagation(); removeNode(n.id, n.date_label); }}
                      style={{ color: T.danger, background: "transparent", border: "none", cursor: "pointer", fontSize: 12.5, padding: "4px 10px" }}
                    >
                      删除
                    </button>
                  </div>
                  );
                })}
              </div>
            )}
            {timelineView === "cal" && (
              (() => {
                // 基准月：时间起点所在月（或今天），加上翻页偏移
                let baseY = new Date().getFullYear(), baseM = new Date().getMonth();
                if (currentStory) {
                  const mm = currentStory.time_origin.trim().match(/^(\d{4})[-./年](\d{1,2})[-./月](\d{1,2})日?$/);
                  if (mm) { baseY = +mm[1]; baseM = +mm[2] - 1; }
                }
                baseM += calOffset;
                const y = baseY + Math.floor(baseM / 12);
                const m = ((baseM % 12) + 12) % 12;
                // 节点 → 剧情日期映射（day 节点；插曲不占日期）
                const nodeDates = new Map<number, { y: number; m: number; d: number }>();
                if (currentStory) {
                  const mm = currentStory.time_origin.trim().match(/^(\d{4})[-./年](\d{1,2})[-./月](\d{1,2})日?$/);
                  if (mm) {
                    const base = new Date(+mm[1], +mm[2] - 1, +mm[3]);
                    for (const nn of timelineNodes) {
                      if (nn.node_type === "interlude" || nn.node_type === "flashback") continue;
                      const dayNo = parseInt(nn.date_label.replace(/\D/g, ""), 10);
                      if (!dayNo || dayNo <= 0) continue;
                      const d = new Date(base);
                      d.setDate(d.getDate() + (dayNo - 1));
                      nodeDates.set(nn.id, { y: d.getFullYear(), m: d.getMonth(), d: d.getDate() });
                    }
                  }
                }
                const firstDay = new Date(y, m, 1).getDay();
                const daysInMonth = new Date(y, m + 1, 0).getDate();
                const cells: (number | null)[] = [];
                for (let i = 0; i < firstDay; i++) cells.push(null);
                for (let d = 1; d <= daysInMonth; d++) cells.push(d);
                while (cells.length % 7 !== 0) cells.push(null);
                const nodeForDate = (dd: number) => {
                  for (const [nid, nd] of nodeDates) {
                    if (nd.y === y && nd.m === m && nd.d === dd) {
                      return timelineNodes.find((x) => x.id === nid) || null;
                    }
                  }
                  return null;
                };
                const interludeNodes = timelineNodes.filter((nn) => nn.node_type === "interlude" || nn.node_type === "flashback");
                return (
                  <div>
                    <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 8 }}>
                      <button onClick={() => setCalOffset(calOffset - 1)} style={{ fontSize: 12, padding: "3px 10px", cursor: "pointer", borderRadius: 6, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, fontFamily: "inherit" }}>◀ 上月</button>
                      <span style={{ fontSize: 14, fontWeight: 600, color: T.text1 }}>{y} 年 {m + 1} 月</span>
                      <button onClick={() => setCalOffset(calOffset + 1)} style={{ fontSize: 12, padding: "3px 10px", cursor: "pointer", borderRadius: 6, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, fontFamily: "inherit" }}>下月 ▶</button>
                      <button onClick={() => setCalOffset(0)} style={{ fontSize: 11, padding: "3px 8px", cursor: "pointer", borderRadius: 6, border: "1px dashed " + T.line, background: "transparent", color: T.text3, fontFamily: "inherit" }}>回到起点月</button>
                    </div>
                    <div style={{ display: "grid", gridTemplateColumns: "repeat(7, 1fr)", gap: 4 }}>
                      {["日", "一", "二", "三", "四", "五", "六"].map((w) => (
                        <div key={w} style={{ textAlign: "center", fontSize: 11, color: T.text3, padding: "4px 0" }}>{w}</div>
                      ))}
                      {cells.map((dd, i) => {
                        if (dd === null) return <div key={i} />;
                        const nn = nodeForDate(dd);
                        const cur = node?.id && nodeDates.has(node.id) && nodeDates.get(node.id)!.y === y && nodeDates.get(node.id)!.m === m && nodeDates.get(node.id)!.d === dd;
                        return (
                          <div key={i} onClick={nn ? () => { switchNode(nn.id); setView("story"); } : undefined} style={{
                            minHeight: 54, borderRadius: 8, padding: 5, cursor: nn ? "pointer" : "default",
                            border: `1px solid ${cur ? T.accent : T.lineSoft}`,
                            background: nn ? T.accentSoft : T.panel2,
                            display: "flex", flexDirection: "column", gap: 2, boxSizing: "border-box",
                          }}>
                            <span style={{ fontSize: 11, color: nn ? T.accent : T.text3, fontWeight: nn ? 700 : 400 }}>{dd}</span>
                            {nn && (
                              <>
                                <span style={{ fontSize: 10, color: T.text2, fontWeight: 600 }}>{nn.date_label}</span>
                                {nn.tags && <span style={{ fontSize: 9, color: T.accent, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{nn.tags.split(",").filter(Boolean).slice(0, 2).map((t) => `#${t}`).join(" ")}</span>}
                              </>
                            )}
                          </div>
                        );
                      })}
                    </div>
                    {interludeNodes.length > 0 && (
                      <div style={{ marginTop: 10, display: "flex", flexWrap: "wrap", gap: 6, alignItems: "center" }}>
                        <span style={{ fontSize: 11, color: T.text3 }}>🎭 插曲 / 回忆（不占日期）：</span>
                        {interludeNodes.map((nn) => (
                          <button key={nn.id} onClick={() => { switchNode(nn.id); setView("story"); }} style={{ fontSize: 11, padding: "3px 10px", borderRadius: 12, border: `1px solid ${T.line}`, background: T.panel2, color: T.text2, cursor: "pointer", fontStyle: "italic", fontFamily: "inherit" }}>
                            {nn.date_label}{nn.tags ? ` · #${nn.tags.split(",")[0]}` : ""}
                          </button>
                        ))}
                      </div>
                    )}
                  </div>
                );
              })()
            )}
          </div>
        )}

        {view === "portrait" && (() => {
          const roles = ["全部", ...Array.from(new Set(portraits.map((m) => m.role)))];
          const totalMB = portraits.reduce((a, m) => a + m.size_bytes, 0);
          const fmtTotal = totalMB >= 1048576 ? (totalMB / 1048576).toFixed(1) + " MB" : totalMB >= 1024 ? Math.round(totalMB / 1024) + " KB" : totalMB + " B";
          const recent = portraits.length ? portraits.map((m) => m.date).sort().slice(-1)[0] : "—";
          const filtered = portraits
            .filter((m) => (pRole === "全部" || m.role === pRole) && (!pSearch.trim() || (m.name + m.role + m.tags.join("")).includes(pSearch.trim())))
            .sort((a, b) => pSort === "name" ? a.name.localeCompare(b.name, "zh") : (b.date + String(b.id)).localeCompare(a.date + String(a.id)));
          const groupList = [...Array.from(new Set(portraits.map((m) => m.group).filter(Boolean))).sort((a, b) => a.localeCompare(b, "zh")), "未分组"];
          const gtag = (t: string) => t === "当前场景" || t === "待审核" ? { background: T.accentSoft, color: T.accent }
            : t === "冷色调" || t === "表情差分 ×4" || t === "年代差异" ? { background: "rgba(91,140,255,.14)", color: "#8ab4ff" }
            : t === "已定稿" ? { background: "rgba(55,201,120,.14)", color: "#5fdc9c" }
            : { background: T.panel3, color: T.text2 };
          const selSet = pSel;
          const toggleSel = (id: number) => setPSel((prev) => { const nx = new Set(prev); if (nx.has(id)) nx.delete(id); else nx.add(id); return nx; });
          const chipBtn = (active: boolean) => ({ display: "inline-flex", alignItems: "center", gap: 5, height: 28, padding: "0 11px", borderRadius: 14, border: `1px solid ${T.line}`, background: "#191c22", fontSize: 11.5, color: active ? T.accent : T.text2, cursor: "pointer", fontFamily: "inherit" });
          const fmtBtn = { display: "inline-flex", alignItems: "center", gap: 6, height: 32, padding: "0 14px", borderRadius: 8, fontSize: 12.5, cursor: "pointer", fontFamily: "inherit", border: `1px solid ${T.line}`, background: "#191c22", color: T.text2 };
          const bindModalJSX = pBindOpen && (
                <div style={{ position: "fixed", inset: 0, background: "rgba(8,10,14,.66)", zIndex: 121, display: "flex", alignItems: "center", justifyContent: "center" }} onClick={() => setPBindOpen(false)}>
                  <div style={{ width: 430, maxWidth: "92vw", border: `1px solid #3a3f4b`, borderRadius: 14, background: T.panel, padding: 20, boxShadow: "0 30px 80px rgba(0,0,0,.6)" }} onClick={(e) => e.stopPropagation()}>
                    <div style={{ fontSize: 15, fontWeight: 650, marginBottom: 4 }}>🎯 绑定角色</div>
                    <div style={{ fontSize: 11.5, color: T.text3, marginBottom: 14 }}>将素材绑定到故事列表中的某个角色，方便后续按角色调用立绘。</div>
                    <div style={{ display: "grid", gridTemplateColumns: "72px 1fr", gap: "8px 12px", alignItems: "center" }}>
                      <label style={{ fontSize: 12, color: T.text2 }}>故事</label>
                      <select value={bindStory} onChange={(e) => { const v = Number(e.target.value); setBindStory(v); setBindChar(0); loadBindChars(v); }} style={{ padding: "7px 10px", background: "#191c22", color: T.text1, border: `1px solid ${T.line}`, borderRadius: 8, fontFamily: "inherit", fontSize: 12 }}>
                        <option value={0}>请选择故事…</option>
                        {stories.map((s) => <option key={s.id} value={s.id}>{s.title}</option>)}
                      </select>
                      <label style={{ fontSize: 12, color: T.text2 }}>角色</label>
                      <select value={bindChar} onChange={(e) => setBindChar(Number(e.target.value))} style={{ padding: "7px 10px", background: "#191c22", color: T.text1, border: `1px solid ${T.line}`, borderRadius: 8, fontFamily: "inherit", fontSize: 12 }}>
                        <option value={0}>{bindStory ? "请选择角色…" : "请先选择故事"}</option>
                        {bindChars.map((ch) => <option key={ch.id} value={ch.id}>{ch.name}</option>)}
                      </select>
                    </div>
                    <div style={{ display: "flex", gap: 10, marginTop: 16, justifyContent: "flex-end" }}>
                      <button onClick={doUnbind} style={{ height: 32, padding: "0 14px", borderRadius: 8, border: `1px solid ${T.warn}`, background: "transparent", color: T.warn, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" }}>解除绑定</button>
                      <button onClick={() => setPBindOpen(false)} style={{ height: 32, padding: "0 16px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" }}>取消</button>
                      <button onClick={doBind} style={{ height: 32, padding: "0 18px", borderRadius: 8, border: "none", background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505", fontWeight: 650, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" }}>确认绑定</button>
                    </div>
                  </div>
                </div>
              );
          const roleEditJSX = pRoleEditOpen && (
                <div style={{ position: "fixed", inset: 0, background: "rgba(8,10,14,.66)", zIndex: 125, display: "flex", alignItems: "center", justifyContent: "center" }} onClick={() => setPRoleEditOpen(false)}>
                  <div style={{ width: 430, maxWidth: "92vw", border: `1px solid #3a3f4b`, borderRadius: 14, background: T.panel, padding: 20, boxShadow: "0 30px 80px rgba(0,0,0,.6)" }} onClick={(e) => e.stopPropagation()}>
                    <div style={{ fontSize: 15, fontWeight: 650, marginBottom: 4 }}>✏️ 修改角色</div>
                    <div style={{ fontSize: 11.5, color: T.text3, marginBottom: 14 }}>设置该素材所属的角色（用于素材库按角色筛选）。</div>
                    <input value={pRoleEditVal} onChange={(e) => setPRoleEditVal(e.target.value)} placeholder="所属角色（可空）" style={{ width: "100%", padding: "7px 10px", boxSizing: "border-box" }} />
                    <div style={{ display: "flex", gap: 10, marginTop: 16, justifyContent: "flex-end" }}>
                      <button onClick={() => setPRoleEditOpen(false)} style={{ height: 32, padding: "0 16px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" }}>取消</button>
                      <button onClick={doRoleSave} style={{ height: 32, padding: "0 18px", borderRadius: 8, border: "none", background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505", fontWeight: 650, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" }}>保存</button>
                    </div>
                  </div>
                </div>
              );
          const pGenEditJSX = pGenEditOpen && (
                <div style={{ position: "fixed", inset: 0, background: "rgba(8,10,14,.66)", zIndex: 125, display: "flex", alignItems: "center", justifyContent: "center" }} onClick={() => setPGenEditOpen(false)}>
                  <div style={{ width: 500, maxWidth: "92vw", border: `1px solid #3a3f4b`, borderRadius: 14, background: T.panel, padding: 20, boxShadow: "0 30px 80px rgba(0,0,0,.6)" }} onClick={(e) => e.stopPropagation()}>
                    <div style={{ fontSize: 15, fontWeight: 650, marginBottom: 4 }}>⌨️ 编辑生成参数</div>
                    <div style={{ fontSize: 11.5, color: T.text3, marginBottom: 14 }}>记录该立绘生成时的配方：提示词与关键参数（导入素材可手动填写，AI 生图后自动写入）。</div>
                    <div style={{ fontSize: 11, color: T.text3, marginBottom: 4 }}>提示词（正向描述）</div>
                    <textarea value={pGenForm.prompt} onChange={(e) => setPGenForm({ ...pGenForm, prompt: e.target.value })} rows={4} placeholder="生成该图时喂给模型的正向描述（可空）" style={{ width: "100%", boxSizing: "border-box", background: "#12141a", border: `1px solid ${T.line}`, borderRadius: 8, color: T.text1, fontSize: 12, padding: "8px 10px", fontFamily: "Consolas, 'Courier New', monospace", resize: "vertical", lineHeight: 1.6 }} />
                    <div style={{ fontSize: 11, color: T.text3, marginBottom: 4, marginTop: 10 }}>负向提示词（可空）</div>
                    <textarea value={pGenForm.negative} onChange={(e) => setPGenForm({ ...pGenForm, negative: e.target.value })} rows={2} placeholder="不希望出现的内容，如 多余手指,低质量（可空）" style={{ width: "100%", boxSizing: "border-box", background: "#12141a", border: `1px solid ${T.line}`, borderRadius: 8, color: T.text1, fontSize: 12, padding: "8px 10px", fontFamily: "Consolas, 'Courier New', monospace", resize: "vertical", lineHeight: 1.6 }} />
                    <div style={{ fontSize: 11, color: T.text3, marginBottom: 4, marginTop: 10 }}>关键参数</div>
                    <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr 1fr", gap: 8 }}>
                      <input value={pGenForm.seed} onChange={(e) => setPGenForm({ ...pGenForm, seed: e.target.value })} placeholder="seed 种子" style={{ padding: "7px 10px", minWidth: 0 }} />
                      <input value={pGenForm.steps} onChange={(e) => setPGenForm({ ...pGenForm, steps: e.target.value })} placeholder="steps 步数" style={{ padding: "7px 10px", minWidth: 0 }} />
                      <input value={pGenForm.cfg} onChange={(e) => setPGenForm({ ...pGenForm, cfg: e.target.value })} placeholder="cfg 引导强度" style={{ padding: "7px 10px", minWidth: 0 }} />
                      <input value={pGenForm.model} onChange={(e) => setPGenForm({ ...pGenForm, model: e.target.value })} placeholder="模型" style={{ padding: "7px 10px", minWidth: 0 }} />
                    </div>
                    <div style={{ display: "flex", gap: 10, marginTop: 16, justifyContent: "flex-end" }}>
                      <button onClick={() => setPGenEditOpen(false)} style={{ height: 32, padding: "0 16px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" }}>取消</button>
                      <button onClick={doGenSave} style={{ height: 32, padding: "0 18px", borderRadius: 8, border: "none", background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505", fontWeight: 650, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" }}>保存</button>
                    </div>
                  </div>
                </div>
              );
          const pModalJSX = pModal && (
                <div style={{ position: "fixed", inset: 0, background: "rgba(8,10,14,.66)", zIndex: 120, display: "flex", alignItems: "center", justifyContent: "center" }} onClick={() => setPModal(null)}>
                  <div style={{ width: 430, maxWidth: "92vw", border: `1px solid #3a3f4b`, borderRadius: 14, background: T.panel, padding: 20, boxShadow: "0 30px 80px rgba(0,0,0,.6)" }} onClick={(e) => e.stopPropagation()}>
                    <div style={{ fontSize: 15, fontWeight: 650, marginBottom: 4 }}>{pModal === "import" ? "📤 导入立绘素材" : "＋ 新建立绘素材"}</div>
                    <div style={{ fontSize: 11.5, color: T.text3, marginBottom: 14 }}>{pModal === "import" ? `已选择：${pDraft.srcPath.split(/[\/]/).pop()}` : "创建一条素材记录（无图占位，稍后可导入图片）"}</div>
                    <div style={{ display: "grid", gridTemplateColumns: "72px 1fr", gap: "8px 12px", alignItems: "center" }}>
                      <label style={{ fontSize: 12, color: T.text2 }}>名称 *</label>
                      <input value={pDraft.name} onChange={(e) => setPDraft({ ...pDraft, name: e.target.value })} placeholder="例如：雨中对峙" style={{ padding: "7px 10px" }} />
                      <label style={{ fontSize: 12, color: T.text2 }}>角色</label>
                      <input value={pDraft.role} onChange={(e) => setPDraft({ ...pDraft, role: e.target.value })} placeholder="所属角色（可空）" style={{ padding: "7px 10px" }} />
                      <label style={{ fontSize: 12, color: T.text2 }}>版本</label>
                      <input value={pDraft.ver} onChange={(e) => setPDraft({ ...pDraft, ver: e.target.value })} placeholder="v1" style={{ padding: "7px 10px" }} />
                      {pModal === "create" && (<>
                        <label style={{ fontSize: 12, color: T.text2 }}>比例</label>
                        <input value={pDraft.ratio} onChange={(e) => setPDraft({ ...pDraft, ratio: e.target.value })} placeholder="如 3:4 / 1024×1536（可空）" style={{ padding: "7px 10px" }} />
                      </>)}
                      <label style={{ fontSize: 12, color: T.text2 }}>标签</label>
                      <input value={pDraft.tags} onChange={(e) => setPDraft({ ...pDraft, tags: e.target.value })} placeholder="逗号分隔，如 全身,冷色调" style={{ padding: "7px 10px" }} />
                      <label style={{ fontSize: 12, color: T.text2 }}>分组</label>
                      <input value={pDraft.group} onChange={(e) => setPDraft({ ...pDraft, group: e.target.value })} placeholder="所属分组（可空，新名自动创建）" style={{ padding: "7px 10px" }} />
                    </div>
                    <div style={{ display: "flex", gap: 10, marginTop: 16, justifyContent: "flex-end" }}>
                      <button onClick={() => setPModal(null)} style={{ height: 32, padding: "0 16px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, cursor: "pointer", fontSize: 12.5 }}>取消</button>
                      <button onClick={submitPModal} style={{ height: 32, padding: "0 18px", borderRadius: 8, border: "none", background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505", fontWeight: 650, cursor: "pointer", fontSize: 12.5 }}>{pModal === "import" ? "导入" : "创建"}</button>
                    </div>
                  </div>
                </div>
              );

          // ===== 详情页（方案 F） =====
          if (pDetail !== null) {
            const m = portraits.find((x) => x.id === pDetail)!;
            const sameRole = portraits.filter((x) => x.role === m.role && x.id !== m.id).slice(0, 4);
            return (
              <>
              <div style={{ maxWidth: 1312, width: "100%", margin: "0 auto", border: `1px solid #3a3f4b`, borderRadius: 10, background: T.win, overflow: "hidden", boxShadow: "0 30px 80px rgba(0,0,0,.55), 0 4px 14px rgba(0,0,0,.4)" }}>
                <div style={{ padding: "14px 24px", background: T.panel, borderBottom: `1px solid ${T.line}`, display: "flex", alignItems: "center", gap: 12 }}>
                  <button onClick={() => setPDetail(null)} style={fmtBtn}>← 返回素材库</button>
                  <span style={{ fontSize: 13, color: T.text2 }}>素材详情 · {m.name}</span>
                </div>
                <div style={{ display: "flex", gap: 16, padding: 16 }}>
                  <div style={{ flex: 1, borderRight: `1px solid ${T.line}`, paddingRight: 16, display: "flex", flexDirection: "column", gap: 12, minWidth: 0 }}>
                    <div style={{ flex: 1, minHeight: 290, borderRadius: 12, position: "relative", overflow: "hidden", display: "flex", alignItems: "center", justifyContent: "center", background: "radial-gradient(circle at 50% 35%,rgba(255,122,24,.12),transparent 62%),linear-gradient(160deg,#4a5f8a,#1e2430)" }}>
                      <div style={{ position: "absolute", left: 10, top: 10, display: "flex", gap: 6 }}>
                        <span style={{ fontSize: 9.5, padding: "1px 6px", borderRadius: 5, background: T.accentSoft, color: T.accent }}>当前场景使用中</span>
                        <span style={{ fontSize: 9.5, padding: "1px 6px", borderRadius: 5, background: "rgba(0,0,0,.55)", color: "#dfe3ec" }}>{m.ver}</span>
                        <span style={{ fontSize: 9.5, padding: "1px 6px", borderRadius: 5, background: "rgba(0,0,0,.55)", color: "#dfe3ec" }}>透明底 PNG</span>
                      </div>
                      {m.path ? <img src={convertFileSrc(m.path)} alt={m.name} style={{ position: "absolute", inset: 0, width: "100%", height: "100%", objectFit: "contain", display: "block" }} /> : <span style={{ fontSize: 96, color: "rgba(255,255,255,.62)" }}>🧑</span>}
                      <div style={{ position: "absolute", right: 10, bottom: 10, display: "flex", gap: 6 }}>
                        {["🔍-", "🔍+", "▦", "⛶"].map((z, i) => <span key={i} title="预览操作（后置）" style={{ width: 28, height: 28, borderRadius: 7, background: "rgba(0,0,0,.55)", display: "flex", alignItems: "center", justifyContent: "center", cursor: "pointer", fontSize: 12, color: "#fff" }}>{z}</span>)}
                      </div>
                    </div>
                    <div style={{ display: "flex", gap: 8 }}>
                      {sameRole.map((x) => <div key={x.id} onClick={() => setPDetail(x.id)} style={{ width: 56, height: 70, borderRadius: 8, overflow: "hidden", border: `1px solid ${x.id === m.id ? T.accent : T.line}`, cursor: "pointer", display: "flex", alignItems: "center", justifyContent: "center", fontSize: 20, background: PORTRAIT_GRADS[x.g] }}>{x.path ? <img src={convertFileSrc(x.path)} alt="" style={{ width: "100%", height: "100%", objectFit: "cover", display: "block" }} /> : <span>🧑</span>}</div>)}
                      <div onClick={() => doPick(m.role)} title="导入同角色素材" style={{ width: 56, height: 70, borderRadius: 8, border: `1px dashed ${T.line}`, display: "flex", alignItems: "center", justifyContent: "center", fontSize: 20, color: T.text3, cursor: "pointer" }}>＋</div>
                    </div>
                  </div>
                  <div style={{ width: 296, flex: "0 0 296px", padding: 16, overflowY: "auto" }}>
                    <div style={{ fontSize: 15, fontWeight: 650, display: "flex", alignItems: "center", gap: 7 }}>{m.name} <span style={{ fontSize: 10, padding: "1px 6px", borderRadius: 5, background: T.accentSoft, color: T.accent }}>已定稿</span></div>
                    <div style={{ fontSize: 11, color: T.text3, marginTop: 4, lineHeight: 1.6 }}>角色：{m.role || "未设置角色"}</div>
                    <div style={{ marginTop: 16 }}>
                      <div style={{ fontSize: 10.5, color: T.text3, letterSpacing: 1, marginBottom: 8, display: "flex", alignItems: "center", gap: 5 }}>ℹ️ 素材信息</div>
                      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", fontSize: 11.5, padding: "6px 0", borderBottom: `1px dashed ${T.lineSoft}`, flexWrap: "wrap", gap: 6 }}>
                        <span style={{ color: T.text3 }}>绑定角色</span>
                        {m.bound_story && m.bound_character ? <span style={{ color: T.accent, fontWeight: 500 }}>{m.bound_story} · {m.bound_character}</span> : <span style={{ color: T.text3 }}>未绑定</span>}
                        <button onClick={() => { setBindStory(m.story_id || 0); setBindChar(m.character_id || 0); if (m.story_id) loadBindChars(m.story_id); setPBindOpen(true); }} style={{ marginLeft: "auto", height: 24, padding: "0 10px", borderRadius: 6, border: `1px solid ${T.line}`, background: "#191c22", color: m.bound_story ? T.accent : T.text2, cursor: "pointer", fontSize: 11, fontFamily: "inherit" }}>{m.bound_story ? "重新绑定" : "绑定"}</button>
                      </div>
                      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", fontSize: 11.5, padding: "6px 0", borderBottom: `1px dashed ${T.lineSoft}`, flexWrap: "wrap", gap: 6 }}>
                        <span style={{ color: T.text3 }}>角色</span>
                        <span style={{ color: T.text1, fontWeight: 500 }}>{m.role || "未设置"}</span>
                        <button onClick={() => { setPRoleEditVal(m.role); setPRoleEditOpen(true); }} style={{ marginLeft: "auto", height: 24, padding: "0 10px", borderRadius: 6, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, cursor: "pointer", fontSize: 11, fontFamily: "inherit" }}>修改</button>
                      </div>
                      <div style={{ display: "flex", justifyContent: "space-between", fontSize: 11.5, padding: "5px 0", borderBottom: `1px dashed ${T.lineSoft}` }}><span style={{ color: T.text3 }}>分组</span><span style={{ color: T.text1, fontWeight: 500 }}>{m.group || "未分组"}</span></div>
                      {[["分辨率", m.ratio], ["文件格式", "PNG · 透明底"], ["体积", m.size], ["创建时间", "2026-09-18"], ["最近更新", "2026-09-25 14:12"]].map(([k, v]) => (
                        <div key={k} style={{ display: "flex", justifyContent: "space-between", fontSize: 11.5, padding: "5px 0", borderBottom: `1px dashed ${T.lineSoft}` }}><span style={{ color: T.text3 }}>{k}</span><span style={{ color: T.text1, fontWeight: 500 }}>{v}</span></div>
                      ))}
                    </div>
                    <div style={{ marginTop: 16 }}>
                      <div style={{ fontSize: 10.5, color: T.text3, letterSpacing: 1, marginBottom: 8 }}>😀 表情差分 · 4</div>
                      <div style={{ display: "grid", gridTemplateColumns: "repeat(4,1fr)", gap: 7 }}>
                        {["冷静", "愠怒", "疲惫", "苦笑"].map((e, i) => (
                          <div key={e} style={{ height: 52, borderRadius: 8, border: `1px solid ${i === 0 ? T.accent : T.line}`, position: "relative", display: "flex", alignItems: "center", justifyContent: "center", fontSize: 19, background: PORTRAIT_GRADS[m.g] }}>😐<span style={{ position: "absolute", bottom: 2, left: 0, right: 0, textAlign: "center", fontSize: 8.5, fontStyle: "normal", color: "#cfd4de", textShadow: "0 1px 2px rgba(0,0,0,.7)" }}>{e}</span></div>
                        ))}
                      </div>
                    </div>
                    <div style={{ marginTop: 16 }}>
                      <div style={{ fontSize: 10.5, color: T.text3, letterSpacing: 1, marginBottom: 8 }}>🎨 主色卡</div>
                      <div style={{ display: "flex", gap: 7 }}>
                        {["#4a5f8a", "#22293a", "#c8b79a", "#7d3b34", "#e8eaf0"].map((s) => <div key={s} style={{ width: 26, height: 26, borderRadius: 7, border: `1px solid rgba(255,255,255,.14)`, background: s }} />)}
                      </div>
                    </div>
                    <div style={{ marginTop: 16 }}>
                      <div style={{ fontSize: 10.5, color: T.text3, letterSpacing: 1, marginBottom: 8, display: "flex", alignItems: "center", gap: 5 }}>⌨️ 生成参数
                        <button onClick={() => { setPGenForm({ prompt: m.prompt, negative: m.negative, seed: m.seed, steps: m.steps, cfg: m.cfg, model: m.model }); setPGenEditOpen(true); }} style={{ marginLeft: "auto", height: 24, padding: "0 10px", borderRadius: 6, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, cursor: "pointer", fontSize: 11, fontFamily: "inherit" }}>编辑</button>
                      </div>
                      {m.prompt || m.seed || m.steps || m.cfg || m.model ? (
                        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                          {m.prompt && <div style={{ background: "#12141a", border: `1px solid ${T.line}`, borderRadius: 8, padding: "9px 11px", fontSize: 10.5, lineHeight: 1.75, color: "#9fb0c9", maxHeight: 92, overflowY: "auto", fontFamily: "Consolas, 'Courier New', monospace", whiteSpace: "pre-wrap", wordBreak: "break-word" }}>{m.prompt}</div>}
                          {m.negative && <div style={{ fontSize: 10, color: "#7c8798", lineHeight: 1.6 }}>负向：{m.negative}</div>}
                          <div style={{ display: "flex", gap: 5, flexWrap: "wrap" }}>
                            {[m.seed && `seed ${m.seed}`, m.steps && `steps ${m.steps}`, m.cfg && `cfg ${m.cfg}`, m.model].filter(Boolean).map((t, i) => (
                              <span key={i} style={{ fontSize: 9.5, padding: "2px 7px", borderRadius: 5, background: "rgba(255,122,24,.1)", border: "1px solid rgba(255,122,24,.28)", color: T.accent }}>{t}</span>
                            ))}
                          </div>
                        </div>
                      ) : (
                        <div style={{ fontSize: 11.5, color: T.text3, padding: "10px 0 4px", lineHeight: 1.7 }}>暂无生成参数<br /><span style={{ fontSize: 10, color: "#5b6270" }}>导入素材可手动填写；AI 生图后自动记录。</span></div>
                      )}
                    </div>
                    <div style={{ marginTop: 16 }}>
                      <div style={{ fontSize: 10.5, color: T.text3, letterSpacing: 1, marginBottom: 8 }}>🕘 版本历史</div>
                      {m.versions && m.versions.length > 0 ? (
                        <div style={{ position: "relative", paddingLeft: 14 }}>
                          <div style={{ position: "absolute", left: 3, top: 5, bottom: 5, width: 1.5, background: T.line }} />
                          {[...m.versions].reverse().map((v) => (
                            <div key={v.version} style={{ position: "relative", paddingBottom: 11 }}>
                              <div style={{ position: "absolute", left: -14, top: 4, width: 8, height: 8, borderRadius: "50%", background: v.version === m.versions[m.versions.length - 1].version ? T.accent : "#3a404c", border: "1.5px solid transparent" }} />
                              <b style={{ fontSize: 11.5, fontWeight: 600, display: "block", color: v.version === m.versions[m.versions.length - 1].version ? T.accent : T.text1 }}>v{v.version}{v.version === m.versions[m.versions.length - 1].version ? " · 当前版本" : ""}</b>
                              <span style={{ fontSize: 10, color: T.text3, marginTop: 2, display: "block" }}>{v.date} · {v.desc}</span>
                            </div>
                          ))}
                        </div>
                      ) : (
                        <div style={{ fontSize: 11.5, color: T.text3, padding: "10px 0 4px" }}>暂无版本历史</div>
                      )}
                    </div>
                  </div>
                </div>
              </div>
              {bindModalJSX}
              {roleEditJSX}
              {pGenEditJSX}
              {pModalJSX}
              </>
            );
          }

          // ===== 素材库主页面 =====
          return (
            <div style={{ maxWidth: 1312, width: "100%", margin: "0 auto", border: `1px solid #3a3f4b`, borderRadius: 10, background: T.win, overflow: "hidden", boxShadow: "0 30px 80px rgba(0,0,0,.55), 0 4px 14px rgba(0,0,0,.4)" }}>
              <div style={{ padding: "14px 24px 0", background: T.panel, borderBottom: `1px solid ${T.line}` }}>
                <div style={{ display: "flex", alignItems: "flex-start", gap: 16 }}>
                  <div>
                    <div style={{ fontSize: 17, fontWeight: 650, display: "flex", alignItems: "center", gap: 9 }}><span style={{ fontSize: 19, color: T.accent }}>🖼</span>立绘素材</div>
                    <div style={{ fontSize: 11.5, color: T.text3, marginTop: 5 }}>共 {portraits.length} 张 · 覆盖 {roles.length - 1} 个角色 · 占用 {fmtTotal} · 最近更新 {recent}</div>
                  </div>
                  <div style={{ marginLeft: "auto", display: "flex", gap: 8, alignItems: "center" }}>
                    <button style={fmtBtn} onClick={() => setMsg("✨ AI 生成立绘：依赖插件系统（后置）")}>✨ AI 生成立绘</button>
                    <button style={fmtBtn} onClick={() => doPick()}>📤 导入本地</button>
                    <button style={{ ...fmtBtn, background: "linear-gradient(135deg,#FF7A18,#ff9a45)", border: "none", color: "#231505", fontWeight: 650, boxShadow: "0 4px 14px rgba(255,122,24,.24)" }} onClick={() => { setPDraft({ srcPath: "", name: "", role: "", ver: "v1", tags: "", ratio: "", group: "" }); setPModal("create"); }}>＋ 新建素材</button>
                  </div>
                </div>
                <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "12px 0 13px", flexWrap: "wrap" }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 7, height: 32, padding: "0 11px", width: 230, border: `1px solid ${T.line}`, borderRadius: 8, background: "#191c22", color: T.text3, fontSize: 12 }}>
                    <span style={{ fontSize: 16 }}>🔍</span>
                    <input value={pSearch} onChange={(e) => setPSearch(e.target.value)} placeholder="搜索素材名、角色或标签" style={{ border: "none", background: "transparent", outline: "none", color: T.text1, fontSize: 12, fontFamily: "inherit", width: "100%" }} />
                  </div>
                  {roles.map((r) => (
                    <button key={r} onClick={() => setPRole(r)} style={chipBtn(pRole === r)}>👥 {r}</button>
                  ))}
                  <button style={chipBtn(false)} onClick={() => setMsg("🏷 标签筛选：后置（规划中）")}>🏷 标签：全身</button>
                  <button style={chipBtn(false)} onClick={() => setMsg("😀 表情差分筛选：后置（规划中）")}>😀 表情差分</button>
                  <button style={chipBtn(false)} onClick={() => setPSort(pSort === "date" ? "name" : "date")}>↕ 排序：{pSort === "date" ? "最近更新" : "名称"}</button>
                  <div style={{ display: "inline-flex", background: "#191c22", border: `1px solid ${T.line}`, borderRadius: 8, padding: 2, marginLeft: "auto" }}>
                    {([["grid", "▦ 网格"], ["list", "☰ 列表"], ["masonry", "≡ 瀑布"], ["group", "🗂 分组"]] as const).map(([v, l]) => (
                      <button key={v} onClick={() => setPView(v)} style={{ border: "none", background: "transparent", color: pView === v ? T.accent : T.text3, fontSize: 11.5, padding: "4px 10px", borderRadius: 6, cursor: "pointer", fontFamily: "inherit", display: "flex", alignItems: "center", gap: 4, fontWeight: pView === v ? 600 : 400 }}>{l}</button>
                    ))}
                  </div>
                </div>
              </div>

              <div style={{ padding: "22px 24px 40px", minHeight: 420 }}>
                {/* 方案 A：标准网格 */}
                {pView === "grid" && (
                  <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(205px, 1fr))", gap: 14 }}>
                    {filtered.map((m) => {
                      const on = selSet.has(m.id);
                      return (
                        <div key={m.id} onClick={() => setPDetail(m.id)} style={{ border: `1px solid ${on ? T.accent : T.line}`, borderRadius: 12, background: "#1e222a", overflow: "hidden", cursor: "pointer", transition: ".16s", boxShadow: on ? "0 0 0 2px rgba(255,122,24,.22)" : "none" }}>
                          <div style={{ height: 170, position: "relative", display: "flex", alignItems: "center", justifyContent: "center", overflow: "hidden", background: PORTRAIT_GRADS[m.g] }}>
                            {m.path ? <img src={convertFileSrc(m.path)} alt={m.name} style={{ position: "absolute", inset: 0, width: "100%", height: "100%", objectFit: "cover", display: "block" }} /> : <span style={{ fontSize: 38, color: "rgba(255,255,255,.72)" }}>🧑</span>}
                            <span style={{ position: "absolute", top: 7, right: 7, fontSize: 9.5, padding: "1px 6px", borderRadius: 5, background: "rgba(0,0,0,.55)", color: "#ffd7ad" }}>{m.ver}</span>
                            <span style={{ position: "absolute", bottom: 7, left: 7, fontSize: 9.5, padding: "1px 6px", borderRadius: 5, background: "rgba(0,0,0,.5)", color: "#dfe3ec" }}>{m.role}</span>
                            <span onClick={(e) => { e.stopPropagation(); toggleSel(m.id); }} style={{ position: "absolute", top: 7, left: 7, width: 18, height: 18, borderRadius: 5, border: "1.5px solid rgba(255,255,255,.55)", background: on ? T.accent : "rgba(0,0,0,.35)", display: "flex", alignItems: "center", justifyContent: "center", fontSize: 12, color: on ? "#231505" : "#fff", cursor: "pointer" }}>{on ? "✓" : ""}</span>
                          </div>
                          <div style={{ padding: "9px 10px 10px" }}>
                            <div style={{ fontSize: 12.5, fontWeight: 600, display: "flex", alignItems: "center", gap: 5 }}>{m.name}{m.pinned && <span style={{ fontSize: 13 }}>📌</span>}</div>
                            <div style={{ fontSize: 11, color: T.text3, marginTop: 3 }}>{m.ratio} · {m.size} · {m.date}</div>
                            <div style={{ display: "flex", flexWrap: "wrap", gap: 4, marginTop: 8 }}>
                              {m.tags.map((t) => <span key={t} style={{ fontSize: 9.5, padding: "2px 6px", borderRadius: 5, ...gtag(t) }}>{t}</span>)}
                            </div>
                          </div>
                        </div>
                      );
                    })}
                    <div style={{ border: `1px dashed ${T.line}`, borderRadius: 12, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 6, color: T.text3, fontSize: 11.5, cursor: "pointer", minHeight: 246, background: "transparent" }} onClick={() => doPick()}>
                      <span style={{ fontSize: 24, color: T.text3 }}>＋</span>上传或生成立绘
                      <span style={{ fontSize: 10, color: "#5b6270" }}>支持 PNG / WEBP，或拖入此区域</span>
                    </div>
                  </div>
                )}

                {/* 方案 C：列表 */}
                {pView === "list" && (
                  <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, overflow: "hidden", background: T.panel }}>
                    <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "9px 14px", borderBottom: `1px solid ${T.lineSoft}`, background: T.panel2, fontSize: 10.5, color: T.text3, letterSpacing: .5 }}>
                      <span style={{ width: 16 }}></span><span style={{ width: 38 }}></span>
                      <span style={{ flex: 1 }}>素材名称</span><span style={{ width: 96 }}>所属角色</span>
                      <span style={{ flex: 1 }}>标签</span><span style={{ width: 96 }}>规格 / 体积</span>
                      <span style={{ width: 88 }}>更新时间</span><span style={{ width: 88, textAlign: "right" }}>操作</span>
                    </div>
                    {filtered.map((m) => {
                      const on = selSet.has(m.id);
                      return (
                        <div key={m.id} onClick={() => setPDetail(m.id)} style={{ display: "flex", alignItems: "center", gap: 12, padding: "9px 14px", borderBottom: `1px solid ${T.lineSoft}`, cursor: "pointer", background: on ? "rgba(255,122,24,.07)" : "transparent", boxShadow: on ? "inset 2px 0 0 #FF7A18" : "none" }}>
                          <span onClick={(e) => { e.stopPropagation(); toggleSel(m.id); }} style={{ width: 16, height: 16, flex: "0 0 16px", borderRadius: 4, border: "1.5px solid #454b58", background: on ? T.accent : "transparent", display: "flex", alignItems: "center", justifyContent: "center", fontSize: 11, color: "#231505", cursor: "pointer" }}>{on ? "✓" : ""}</span>
                          <div style={{ width: 38, height: 50, flex: "0 0 38px", borderRadius: 6, overflow: "hidden", display: "flex", alignItems: "center", justifyContent: "center", fontSize: 18, color: "rgba(255,255,255,.7)", background: PORTRAIT_GRADS[m.g] }}>{m.path ? <img src={convertFileSrc(m.path)} alt="" style={{ width: "100%", height: "100%", objectFit: "cover", display: "block" }} /> : <span>🧑</span>}</div>
                          <div style={{ flex: 1, minWidth: 0 }}><b style={{ fontSize: 12.5, fontWeight: 600 }}>{m.name}</b><i style={{ fontSize: 10.5, color: T.text3, fontStyle: "normal", display: "block" }}>{m.ver}{m.pinned ? " · 已置顶" : ""}</i></div>
                          <div style={{ width: 96, flex: "0 0 96px", fontSize: 11.5, color: T.text2, display: "flex", alignItems: "center", gap: 6 }}><span style={{ width: 18, height: 18, borderRadius: "50%", flex: "0 0 18px", background: PORTRAIT_GRADS[m.g] }} />{m.role}</div>
                          <div style={{ flex: 1, minWidth: 0, display: "flex", gap: 4, flexWrap: "wrap" }}>{m.tags.map((t) => <span key={t} style={{ fontSize: 9.5, padding: "2px 6px", borderRadius: 5, ...gtag(t) }}>{t}</span>)}</div>
                          <div style={{ width: 96, flex: "0 0 96px", fontSize: 10.5, color: T.text3, lineHeight: 1.5 }}>{m.ratio}<br />{m.size}</div>
                          <div style={{ width: 88, flex: "0 0 88px", fontSize: 10.5, color: T.text3 }}>{m.date} 14:12</div>
                          <div style={{ width: 88, flex: "0 0 88px", display: "flex", gap: 2, justifyContent: "flex-end" }}>
                            <span style={{ fontSize: 16, color: T.text3, cursor: "pointer", padding: 4, borderRadius: 5 }}>👁</span>
                            <span style={{ fontSize: 16, color: T.text3, cursor: "pointer", padding: 4, borderRadius: 5 }}>✎</span>
                            <span style={{ fontSize: 16, color: T.text3, cursor: "pointer", padding: 4, borderRadius: 5 }}>⋯</span>
                          </div>
                        </div>
                      );
                    })}
                  </div>
                )}

                {/* 方案 D：瀑布流 */}
                {pView === "masonry" && (
                  <div style={{ display: "grid", gridTemplateColumns: "repeat(3,1fr)", gap: 14, alignItems: "start" }}>
                    {[0, 1, 2].map((col) => (
                      <div key={col} style={{ display: "flex", flexDirection: "column", gap: 14 }}>
                        {filtered.filter((_, i) => i % 3 === col).map((m) => {
                          const h = m.name.includes("全身") ? 250 : m.name.includes("半身") ? 180 : m.name.includes("场景") ? 150 : 120;
                          const rt = m.name.includes("场景") ? "场景 16:9" : m.name.includes("半身") ? "半身 4:5" : m.name.includes("特写") ? "特写 1:1" : "全身 3:4";
                          return (
                            <div key={m.id} onClick={() => setPDetail(m.id)} style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: "#1e222a", overflow: "hidden", cursor: "pointer" }}>
                              <div style={{ height: h, position: "relative", display: "flex", alignItems: "center", justifyContent: "center", background: PORTRAIT_GRADS[m.g] }}>
                                {m.path ? <img src={convertFileSrc(m.path)} alt={m.name} style={{ position: "absolute", inset: 0, width: "100%", height: "100%", objectFit: "cover", display: "block" }} /> : <span style={{ fontSize: 34, color: "rgba(255,255,255,.72)" }}>🧑</span>}
                                <span style={{ position: "absolute", top: 7, left: 7, fontSize: 9.5, padding: "1px 6px", borderRadius: 5, background: "rgba(0,0,0,.55)", color: "#dfe3ec" }}>{rt}</span>
                              </div>
                              <div style={{ padding: "8px 9px", fontSize: 11.5, fontWeight: 600 }}>{m.name}<span style={{ display: "block", fontSize: 10, color: T.text3, fontWeight: 400, marginTop: 2 }}>{m.role} · {m.ver} · {m.ratio}</span></div>
                            </div>
                          );
                        })}
                      </div>
                    ))}
                  </div>
                )}

                {/* 分组视图：按 group_name 聚合，未分组兜底；组内跨角色（角色筛选先生效） */}
                {pView === "group" && (
                  <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
                    {groupList.map((g) => {
                      const items = filtered.filter((m) => (g === "未分组" ? !m.group : m.group === g));
                      if (items.length === 0) return null;
                      return (
                        <div key={g} style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: "#1e222a", overflow: "hidden" }}>
                          <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "11px 14px", background: T.panel2, borderBottom: `1px solid ${T.lineSoft}` }}>
                            <div style={{ width: 30, height: 30, borderRadius: 9, display: "flex", alignItems: "center", justifyContent: "center", fontSize: 16, color: "rgba(255,255,255,.85)", background: g === "未分组" ? "#31363f" : T.accentSoft }}>{g === "未分组" ? "🗃" : "🗂"}</div>
                            <div>
                              <div style={{ fontSize: 12.5, fontWeight: 650, display: "flex", alignItems: "center", gap: 6 }}>{g} {g === "未分组" && <span style={{ fontSize: 9.5, padding: "1px 6px", borderRadius: 5, background: T.panel3, color: T.text3 }}>兜底</span>}</div>
                              <div style={{ fontSize: 10.5, color: T.text3, marginTop: 2 }}>{items.length} 张素材 · 最近更新 {items[0].date}</div>
                            </div>
                            <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 8 }}>
                              <span style={chipBtn(false)} onClick={() => doPick("", g === "未分组" ? "" : g)}>＋ 添加</span>
                              <span style={{ fontSize: 14, color: T.text3, cursor: "pointer" }}>⌄</span>
                            </div>
                          </div>
                          <div style={{ display: "flex", gap: 10, padding: "12px 14px", overflowX: "auto" }}>
                            {items.map((m) => {
                              const cur = m.tags.includes("当前场景");
                              return (
                                <div key={m.id} onClick={() => setPDetail(m.id)} style={{ width: 74, flex: "0 0 74px", cursor: "pointer" }}>
                                  <div style={{ height: 96, borderRadius: 9, overflow: "hidden", border: `1px solid ${cur ? T.accent : T.line}`, boxShadow: cur ? "0 0 0 2px rgba(255,122,24,.2)" : "none", background: PORTRAIT_GRADS[m.g], display: "flex", alignItems: "center", justifyContent: "center", fontSize: 24 }}>{m.path ? <img src={convertFileSrc(m.path)} alt="" style={{ width: "100%", height: "100%", objectFit: "cover", display: "block" }} /> : <span>🧑</span>}</div>
                                  <div style={{ textAlign: "center", marginTop: 5 }}>
                                    {m.role && <span style={{ fontSize: 8.5, padding: "0 5px", borderRadius: 4, background: T.accentSoft, color: T.accent, display: "inline-block", maxWidth: 70, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{m.role}</span>}
                                    <p style={{ fontSize: 10, color: T.text2, margin: "3px 0 0", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{m.name.split(" · ")[0]}</p>
                                  </div>
                                </div>
                              );
                            })}
                            <div style={{ width: 74, flex: "0 0 74px", border: `1px dashed ${T.line}`, borderRadius: 9, display: "flex", alignItems: "center", justifyContent: "center", fontSize: 20, color: T.text3, cursor: "pointer" }} onClick={() => doPick("", g === "未分组" ? "" : g)}>＋</div>
                          </div>
                        </div>
                      );
                    })}
                  </div>
                )}

                {/* 方案 G：空态 */}
                {filtered.length === 0 && (
                  <div style={{ border: `1px dashed ${T.line}`, borderRadius: 12, padding: "26px 12px", textAlign: "center", background: "transparent" }}>
                    <div style={{ width: 56, height: 56, borderRadius: 16, background: T.panel3, display: "flex", alignItems: "center", justifyContent: "center", margin: "0 auto 12px", fontSize: 26, color: T.text3 }}>🖼</div>
                    <div style={{ fontSize: 12.5, fontWeight: 600 }}>{portraits.length === 0 ? "还没有立绘素材" : "没有匹配的立绘素材"}</div>
                    <p style={{ fontSize: 11, color: T.text3, marginTop: 5, lineHeight: 1.6 }}>{portraits.length === 0 ? "点右上角「📤 导入本地」导入图片，或「＋ 新建素材」创建占位记录。" : "试试调整搜索词或角色筛选。"}</p>
                  </div>
                )}
              </div>

              {/* 方案 B：批量操作条 */}
              {selSet.size > 0 && (
                <div style={{ position: "fixed", left: 216, right: 24, bottom: 14, zIndex: 90, marginTop: 14, border: `1px solid rgba(255,122,24,.35)`, background: "rgba(255,122,24,.08)", borderRadius: 12, padding: "10px 14px", display: "flex", alignItems: "center", gap: 10, backdropFilter: "blur(4px)" }}>
                  <span style={{ fontSize: 12, color: T.accent, fontWeight: 650 }}>已选 {selSet.size} 项</span>
                  <span style={{ flex: 1 }} />
                  <button style={fmtBtn} onClick={() => setPGroupOpen(true)}>🗂 移动到分组</button>
                  <button style={fmtBtn} onClick={() => setPTagOpen(true)}>🏷 批量打标签</button>
                  <button style={fmtBtn} onClick={doPExport}>⬇ 导出</button>
                  <button style={{ ...fmtBtn, borderColor: "rgba(240,85,77,.4)", color: "#ff8c85" }} onClick={() => setPDelOpen(true)}>🗑 删除</button>
                  <button style={fmtBtn} onClick={() => setPSel(new Set())}>取消选择</button>
                </div>
              )}

              {/* 导入 / 新建素材弹窗 */}
              {pModalJSX}

              {/* 移动到分组弹窗 */}
              {pGroupOpen && (
                <div style={{ position: "fixed", inset: 0, background: "rgba(8,10,14,.66)", zIndex: 122, display: "flex", alignItems: "center", justifyContent: "center" }} onClick={() => setPGroupOpen(false)}>
                  <div style={{ width: 430, maxWidth: "92vw", border: `1px solid #3a3f4b`, borderRadius: 14, background: T.panel, padding: 20, boxShadow: "0 30px 80px rgba(0,0,0,.6)" }} onClick={(e) => e.stopPropagation()}>
                    <div style={{ fontSize: 15, fontWeight: 650, marginBottom: 4 }}>🗂 移动到分组</div>
                    <div style={{ fontSize: 11.5, color: T.text3, marginBottom: 14 }}>为已选 {selSet.size} 项素材设置分组。</div>
                    <div style={{ display: "flex", flexWrap: "wrap", gap: 7, marginBottom: 12 }}>
                      {Array.from(new Set(portraits.map((m) => m.group).filter(Boolean))).map((g) => (
                        <button key={g} onClick={() => setPGroupInput(g)} style={{ display: "inline-flex", alignItems: "center", height: 26, padding: "0 12px", borderRadius: 13, border: `1px solid ${pGroupInput === g ? T.accent : T.line}`, background: pGroupInput === g ? T.accentSoft : "#191c22", color: pGroupInput === g ? T.accent : T.text2, cursor: "pointer", fontSize: 11.5, fontFamily: "inherit" }}>{g}</button>
                      ))}
                    </div>
                    <input value={pGroupInput} onChange={(e) => setPGroupInput(e.target.value)} placeholder="输入分组名（新分组将自动创建）" style={{ width: "100%", padding: "7px 10px", boxSizing: "border-box" }} />
                    <div style={{ display: "flex", gap: 10, marginTop: 16, justifyContent: "flex-end" }}>
                      <button onClick={() => setPGroupOpen(false)} style={{ height: 32, padding: "0 16px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" }}>取消</button>
                      <button onClick={doGroupSet} style={{ height: 32, padding: "0 18px", borderRadius: 8, border: "none", background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505", fontWeight: 650, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" }}>确认移动</button>
                    </div>
                  </div>
                </div>
              )}

              {/* 批量打标签弹窗 */}
              {pTagOpen && (
                <div style={{ position: "fixed", inset: 0, background: "rgba(8,10,14,.66)", zIndex: 123, display: "flex", alignItems: "center", justifyContent: "center" }} onClick={() => setPTagOpen(false)}>
                  <div style={{ width: 430, maxWidth: "92vw", border: `1px solid #3a3f4b`, borderRadius: 14, background: T.panel, padding: 20, boxShadow: "0 30px 80px rgba(0,0,0,.6)" }} onClick={(e) => e.stopPropagation()}>
                    <div style={{ fontSize: 15, fontWeight: 650, marginBottom: 4 }}>🏷 批量打标签</div>
                    <div style={{ fontSize: 11.5, color: T.text3, marginBottom: 14 }}>为已选 {selSet.size} 项素材追加标签（已有标签保留）。</div>
                    <input value={pTagInput} onChange={(e) => setPTagInput(e.target.value)} placeholder="逗号分隔，如 全身,冷色调" style={{ width: "100%", padding: "7px 10px", boxSizing: "border-box" }} />
                    <div style={{ display: "flex", gap: 10, marginTop: 16, justifyContent: "flex-end" }}>
                      <button onClick={() => setPTagOpen(false)} style={{ height: 32, padding: "0 16px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" }}>取消</button>
                      <button onClick={doTagBatch} style={{ height: 32, padding: "0 18px", borderRadius: 8, border: "none", background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505", fontWeight: 650, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" }}>确认添加</button>
                    </div>
                  </div>
                </div>
              )}

              {/* 批量删除确认弹窗 */}
              {pDelOpen && (
                <div style={{ position: "fixed", inset: 0, background: "rgba(8,10,14,.66)", zIndex: 124, display: "flex", alignItems: "center", justifyContent: "center" }} onClick={() => setPDelOpen(false)}>
                  <div style={{ width: 430, maxWidth: "92vw", border: `1px solid #3a3f4b`, borderRadius: 14, background: T.panel, padding: 20, boxShadow: "0 30px 80px rgba(0,0,0,.6)" }} onClick={(e) => e.stopPropagation()}>
                    <div style={{ fontSize: 15, fontWeight: 650, marginBottom: 4 }}>🗑 删除素材</div>
                    <div style={{ fontSize: 11.5, color: T.text3, marginBottom: 14 }}>将永久删除已选 {selSet.size} 项素材及其图片文件，此操作不可恢复。</div>
                    <div style={{ display: "flex", gap: 10, marginTop: 16, justifyContent: "flex-end" }}>
                      <button onClick={() => setPDelOpen(false)} style={{ height: 32, padding: "0 16px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" }}>取消</button>
                      <button onClick={doDelete} style={{ height: 32, padding: "0 18px", borderRadius: 8, border: "none", background: "linear-gradient(135deg,#e0483e,#f06b5a)", color: "#fff", fontWeight: 650, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" }}>确认删除</button>
                    </div>
                  </div>
                </div>
              )}

              {roleEditJSX}
              {bindModalJSX}
            </div>
          );
        })()}
        {view === "params" && (() => {
          const nodeLabel = (id: number | null) => {
            const t = timelineNodes.find((n2) => n2.id === id);
            return t ? t.date_label : (id != null ? `节点 #${id}` : "未知节点");
          };
          const dbg = (pDebugId != null ? debugList.find((d) => d.id === pDebugId) : undefined)
            ?? (node ? debugList.find((d) => d.node_id === node.id) : undefined)
            ?? debugList[0]
            ?? null;
          const { sysSections, tailSection, history, lastUser, priorHistory, sysTokens, histTokens, userTokens, usedTokens, inputBudget, remain, pctUsed } = computeBudget(dbg, ctxWindow);
          const fmt = (n: number) => n.toLocaleString("en-US");
          const paramP = paramPortrait;
          const budgetRows: { name: string; note: string; tokens: number; color: string; cut: string; cutColor: string }[] = [
            ...sysSections.map((s) => ({ name: s.name, note: s.text.slice(0, 24).replace(/\n/g, " "), tokens: estimateTokens(s.text), color: sectionColor(s.name), cut: "不可裁剪", cutColor: T.ok })),
            ...(tailSection ? [{ name: tailSection.name, note: tailSection.text.slice(0, 24).replace(/\n/g, " "), tokens: estimateTokens(tailSection.text), color: "#ff9a45", cut: "不可裁剪", cutColor: T.ok }] : []),
            { name: "历史上下文", note: priorHistory.length ? `${priorHistory.length} 条（不含本次输入）` : "本次未携带历史", tokens: histTokens, color: T.accent, cut: priorHistory.length ? "第 1 顺位裁剪" : "—", cutColor: priorHistory.length ? T.danger : T.text3 },
            { name: "本次输入", note: lastUser ? lastUser.content.slice(0, 24).replace(/\n/g, " ") : "当前用户输入", tokens: userTokens, color: "#ffd7ad", cut: "不可裁剪", cutColor: T.ok },
          ];
          const windowFor = (n: number) => `${Math.min(100, (n / inputBudget) * 100)}%`;
          return (
            <div style={{ maxWidth: 1120, margin: "0 auto" }}>
              <div style={{ display: "flex", alignItems: "flex-start", gap: 12, marginBottom: 14 }}>
                <div>
                  <div style={{ fontSize: 15, fontWeight: 650, display: "flex", alignItems: "center", gap: 8 }}>🎛 生成参数</div>
                  <div style={{ fontSize: 11.5, color: T.text3, marginTop: 4 }}>
                    {currentStory ? `《${currentStory.title}》 · 模型：${modelName || "未配置"} · 上下文窗口 ${fmt(ctxWindow)} token` : "尚未进入故事"}
                    {dbg ? ` · 快照 #${dbg.id} · ${nodeLabel(dbg.node_id)} · N=${dbg.n_used} 条（${dbg.created_at}）` : " · 暂无请求快照"}
                  </div>
                </div>
                <div style={{ marginLeft: "auto", display: "flex", gap: 8, alignItems: "center" }}>
                  {debugList.length > 0 && (
                    <select value={dbg?.id ?? ""} onChange={(e) => setPDebugId(Number(e.target.value))} title="选择请求快照（默认跟随当前时间线节点）" style={{ height: 32, padding: "0 10px", maxWidth: 330 }}>
                      {debugList.map((d) => (
                        <option key={d.id} value={d.id}>#{d.id} · {nodeLabel(d.node_id)} · {d.created_at.slice(5, 16)}</option>
                      ))}
                    </select>
                  )}
                  <button onClick={() => { if (currentStory) loadDebug(); }} disabled={!currentStory} style={{ height: 32, padding: "0 14px", borderRadius: 8, border: `1px solid ${T.line}`, background: T.panel2, color: T.text2, cursor: currentStory ? "pointer" : "not-allowed", fontSize: 12.5 }}>🔄 刷新快照</button>
                </div>
              </div>
              <div style={{ display: "flex", gap: 4, padding: 3, border: `1px solid ${T.line}`, borderRadius: 10, background: T.panel2, width: "fit-content", marginBottom: 16 }}>
                {([["budget", "📊 Token 预算"], ["preview", "🧾 Prompt 预览"], ["image", "🎨 立绘参数"], ["json", "🧩 JSON"]] as const).map(([k, lb]) => (
                  <button key={k} onClick={() => setParamsTab(k)} style={{ border: "none", borderRadius: 7, padding: "7px 14px", fontSize: 12.5, cursor: "pointer", background: paramsTab === k ? T.accentSoft : "transparent", color: paramsTab === k ? T.accent : T.text2, fontWeight: paramsTab === k ? 600 : 400 }}>{lb}</button>
                ))}
              </div>
              {!currentStory && (
                <div style={{ border: `1px dashed ${T.line}`, borderRadius: 12, padding: "52px 20px", textAlign: "center", color: T.text3 }}>
                  <div style={{ fontSize: 26, marginBottom: 8 }}>🎛</div>
                  <div style={{ fontSize: 13 }}>请先进入一个故事，再查看该故事的生成参数。</div>
                  <button onClick={() => setView("stories")} style={{ marginTop: 12, height: 32, padding: "0 16px", borderRadius: 8, border: "none", background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505", cursor: "pointer", fontSize: 12.5, fontWeight: 650 }}>去故事列表</button>
                </div>
              )}
              {currentStory && paramsTab === "budget" && (!dbg ? (
                <div style={{ border: `1px dashed ${T.line}`, borderRadius: 12, padding: "46px 20px", textAlign: "center", color: T.text3, fontSize: 12.5 }}>
                  还没有请求快照。回到剧情区发送一条消息，或点右上角「刷新快照」。
                </div>
              ) : (
                <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
                  <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, padding: 16 }}>
                    <div style={{ display: "flex", alignItems: "flex-end", gap: 14, flexWrap: "wrap" }}>
                      <div style={{ fontSize: 30, fontWeight: 800, letterSpacing: 0.5 }}>{fmt(usedTokens)}<span style={{ fontSize: 12, color: T.text3, fontWeight: 400, marginLeft: 6 }}>/ {fmt(inputBudget)} token 输入预算</span></div>
                      <div style={{ fontSize: 11, color: T.text3, lineHeight: 1.7, paddingBottom: 3 }}>
                        已用 <b style={{ color: pctUsed > 85 ? T.warn : T.text1 }}>{pctUsed}%</b> · 剩余 {fmt(remain)} token<br />
                        上下文窗口 {fmt(dbg.context_window > 0 ? dbg.context_window : ctxWindow)} × 系数 {dbg.reserve_ratio > 0 ? dbg.reserve_ratio : 0.8} − 输出预留 {fmt((dbg.max_tokens > 0 ? dbg.max_tokens : 4000) + (dbg.safety_margin > 0 ? dbg.safety_margin : 800))}<br />
                        快照节点：{nodeLabel(dbg.node_id)} · 实际带 {dbg.n_used} 条消息（含本次输入）
                      </div>
                    </div>
                    <div style={{ display: "flex", height: 20, borderRadius: 6, overflow: "hidden", border: `1px solid ${T.line}`, background: "#12141a", marginTop: 13 }}>
                      {budgetRows.map((r, i) => r.tokens > 0 && (
                        <div key={`${r.name}-${i}`} title={`${r.name} ${fmt(r.tokens)} token`} style={{ width: windowFor(r.tokens), background: r.color, height: "100%", display: "flex", alignItems: "center", justifyContent: "center", fontSize: 9, color: "rgba(0,0,0,.65)", fontWeight: 700, whiteSpace: "nowrap", overflow: "hidden" }}>{r.tokens > inputBudget * 0.08 ? `${r.name} ${fmt(r.tokens)}` : ""}</div>
                      ))}
                      {remain > 0 && <div style={{ flex: 1, background: "#262a33" }} />}
                    </div>
                    <div style={{ display: "flex", flexWrap: "wrap", gap: 12, marginTop: 12, paddingTop: 12, borderTop: `1px dashed ${T.lineSoft}` }}>
                      {budgetRows.map((r, i) => (
                        <span key={`${r.name}-legend-${i}`} style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 10.5, color: T.text2 }}>
                          <i style={{ width: 9, height: 9, borderRadius: 3, background: r.color, display: "block" }} />{r.name} <b style={{ color: T.text1, fontFamily: "monospace", fontSize: 10.5 }}>{fmt(r.tokens)}</b>
                        </span>
                      ))}
                    </div>
                  </div>
                  <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, overflow: "hidden" }}>
                    <div style={{ display: "grid", gridTemplateColumns: "1fr 130px 90px 110px", gap: 10, padding: "9px 14px", background: T.panel2, fontSize: 10.5, color: T.text3, letterSpacing: 0.5 }}>
                      <span>注入内容</span><span>占用比例</span><span style={{ textAlign: "right" }}>Token</span><span style={{ textAlign: "right" }}>裁剪优先级</span>
                    </div>
                    {budgetRows.map((r, i) => (
                      <div key={`${r.name}-row-${i}`} style={{ display: "grid", gridTemplateColumns: "1fr 130px 90px 110px", gap: 10, alignItems: "center", padding: "9px 14px", borderTop: `1px solid ${T.lineSoft}`, fontSize: 11.5 }}>
                        <span style={{ display: "flex", alignItems: "center", gap: 7, minWidth: 0 }}>
                          <i style={{ width: 8, height: 8, borderRadius: 3, background: r.color, flex: "0 0 8px" }} />
                          <span style={{ fontWeight: 600, color: T.text1 }}>{r.name}</span>
                          <span style={{ color: T.text3, fontSize: 10.5, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{r.note}</span>
                        </span>
                        <span style={{ height: 6, borderRadius: 3, background: "#12141a", border: `1px solid ${T.lineSoft}`, overflow: "hidden", display: "block" }}><i style={{ display: "block", height: "100%", width: windowFor(r.tokens), background: r.color }} /></span>
                        <span style={{ textAlign: "right", fontFamily: "monospace", color: T.text2 }}>{fmt(r.tokens)}</span>
                        <span style={{ textAlign: "right" }}><span style={{ fontSize: 9.5, padding: "2px 7px", borderRadius: 5, color: r.cutColor, background: `${r.cutColor}22`, whiteSpace: "nowrap" }}>{r.cut}</span></span>
                      </div>
                    ))}
                  </div>
                  <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, padding: "12px 16px", fontSize: 11.5, color: T.text2, lineHeight: 1.9 }}>
                    <div style={{ fontWeight: 650, color: T.text1, marginBottom: 6 }}>🧮 本次预算口径（后端算一次，前端直接读快照）</div>
                    <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(215px, 1fr))", gap: "2px 18px", fontFamily: "monospace", fontSize: 11 }}>
                      <span>上下文窗口　　{fmt(dbg.context_window > 0 ? dbg.context_window : ctxWindow)}</span>
                      <span>预算系数　　　× {dbg.reserve_ratio > 0 ? dbg.reserve_ratio : 0.8}</span>
                      <span>输出预留　　　− {fmt((dbg.max_tokens > 0 ? dbg.max_tokens : 4000) + (dbg.safety_margin > 0 ? dbg.safety_margin : 800))}（输出 {fmt(dbg.max_tokens > 0 ? dbg.max_tokens : 4000)} + 余量 {fmt(dbg.safety_margin > 0 ? dbg.safety_margin : 800)}）</span>
                      <span style={{ color: T.accent }}>= 输入预算　　{fmt(inputBudget)}</span>
                      <span>system 段合计　{fmt(sysTokens)}</span>
                      <span>其中：历史上下文 {fmt(histTokens)} · 本次输入 {fmt(userTokens)}</span>
                    </div>
                    <div style={{ marginTop: 8, paddingTop: 8, borderTop: `1px dashed ${T.lineSoft}`, fontSize: 11, color: T.text3 }}>
                      system 分段占用：
                      {sysSections.map((s) => ` ${s.name} ${fmt(estimateTokens(s.text))}`).join(" ｜")}
                      {tailSection ? ` ｜ 上下文更新（尾部） ${fmt(estimateTokens(tailSection.text))}` : ""}
                    </div>
                  </div>
                  {/* B 子项：每日 N 值（由预算倒推，不写死）—— 同时给出"当前设置"与"32k 默认档"两种口径 */}
                  <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, padding: "12px 16px", fontSize: 11.5, color: T.text2, lineHeight: 1.9 }}>
                    <div style={{ fontWeight: 650, color: T.text1, marginBottom: 8 }}>📐 每日 N 值（由预算倒推，不写死）</div>
                    {(() => {
                      const outRes = (dbg.max_tokens > 0 ? dbg.max_tokens : 4000) + (dbg.safety_margin > 0 ? dbg.safety_margin : 800);
                      const ratio = dbg.reserve_ratio > 0 ? dbg.reserve_ratio : 0.8;
                      const days = dbg.day_window ?? 0;
                      const avg = dbg.avg_msg_tokens && dbg.avg_msg_tokens > 0 ? dbg.avg_msg_tokens : 300;
                      const cap = Number(nRecentInput) || 10;
                      const win = dbg.context_window > 0 ? dbg.context_window : ctxWindow;
                      // 按给定窗口算：N = (窗口×系数 − 输出预留 − 固定区 − 尾部块 − 本次输入) ÷ 保留天数 ÷ 单条均token
                      const nFor = (w: number) => {
                        if (days <= 0) return { win: w, perDay: 0, nRaw: 0, nFinal: 0, overflow: false, used: 0 };
                        const scaled = Math.round(w * ratio);
                        const used = outRes + sysTokens + estimateTokens(tailSection?.text || "") + userTokens;
                        const hist = scaled - used;
                        // 预算被固定区/预留吃光（小窗口 + 厚重设定时会发生）→ N 只能是 0，单独标出来
                        const overflow = hist <= 0;
                        const safeHist = Math.max(hist, 0);
                        const perDay = Math.floor(safeHist / days);
                        const nRaw = Math.floor(perDay / avg);
                        return { win: w, perDay, nRaw, nFinal: Math.min(nRaw, cap), overflow, used };
                      };
                      const cur = nFor(win);
                      const base = nFor(32768);
                      const perDay = (dbg.day_n || "").split(",").filter(Boolean);
                      const rows: { k: string; a: string; b: string }[] = [
                        { k: "上下文窗口", a: fmt(base.win), b: fmt(cur.win) },
                        { k: "× 安全系数", a: `${ratio}`, b: `${ratio}` },
                        { k: "− 输出预留", a: fmt(outRes), b: fmt(outRes) },
                        { k: "− 固定区（实测）", a: fmt(sysTokens), b: fmt(sysTokens) },
                        { k: "− 尾部块 + 本次输入", a: fmt(estimateTokens(tailSection?.text || "") + userTokens), b: fmt(estimateTokens(tailSection?.text || "") + userTokens) },
                        { k: "÷ 保留天数", a: `${days || "—"}`, b: `${days || "—"}` },
                        { k: "= 每日基准预算", a: fmt(base.perDay), b: fmt(cur.perDay) },
                      ];
                      return (
                        <>
                          <div style={{ display: "grid", gridTemplateColumns: "minmax(150px, 1.4fr) 1fr 1fr", gap: "0 10px", fontFamily: "monospace", fontSize: 11 }}>
                            <span style={{ color: T.text3, borderBottom: `1px solid ${T.lineSoft}`, paddingBottom: 4 }}>推导项</span>
                            <span style={{ color: T.text3, textAlign: "right", borderBottom: `1px solid ${T.lineSoft}`, paddingBottom: 4 }}>默认档 32k</span>
                            <span style={{ color: T.accent, textAlign: "right", borderBottom: `1px solid ${T.lineSoft}`, paddingBottom: 4 }}>当前设置 {fmt(cur.win)}</span>
                            {rows.flatMap((r) => [
                              <span key={`${r.k}-k`} style={{ borderBottom: `1px dashed ${T.lineSoft}`, padding: "2px 0" }}>{r.k}</span>,
                              <span key={`${r.k}-a`} style={{ textAlign: "right", borderBottom: `1px dashed ${T.lineSoft}`, padding: "2px 0" }}>{r.a}</span>,
                              <span key={`${r.k}-b`} style={{ textAlign: "right", borderBottom: `1px dashed ${T.lineSoft}`, padding: "2px 0", color: T.text1 }}>{r.b}</span>,
                            ])}
                            <span style={{ paddingTop: 4 }}>÷ 单条均 token（实测）</span>
                            <span style={{ textAlign: "right", paddingTop: 4 }}>{fmt(avg)}</span>
                            <span style={{ textAlign: "right", paddingTop: 4, color: T.text1 }}>{fmt(avg)}</span>
                          </div>
                          {(base.overflow || cur.overflow) && (
                            <div style={{ marginTop: 6, padding: "6px 9px", borderRadius: 7, background: "rgba(240,85,77,.08)", border: "1px solid rgba(240,85,77,.3)", fontSize: 11, color: "#ff8c85", lineHeight: 1.7 }}>
                              ⚠ {base.overflow ? `默认档 32k 已被固定区吃满（固定区 + 预留 ≈ ${fmt(base.used)} token &gt; ${fmt(Math.round(base.win * ratio))}）：这种设定量下 32k 窗口放不下任何历史，至少要 ${fmt(Math.ceil(base.used / ratio / 1000) * 1000)} 左右才装得下。` : ""}
                              {cur.overflow ? "当前设置的窗口也已被固定区吃满，历史无法携带 —— 请调大上下文窗口。" : ""}
                            </div>
                          )}
                          <div style={{ marginTop: 6, fontSize: 10.5, color: T.text3, lineHeight: 1.7 }}>
                            「固定区」包含：基础指令 / 输出排版 / 数据字典（这三项<b>不变</b>）＋ 世界观 / 角色 / 方向和关系（这三项<b>不一定会变</b>，改动设定后会跟着变）
                          </div>
                          <div style={{ display: "grid", gridTemplateColumns: "minmax(150px, 1.4fr) 1fr 1fr", gap: "0 10px", marginTop: 8, paddingTop: 8, borderTop: `1px solid ${T.line}`, fontSize: 12 }}>
                            <span style={{ fontWeight: 650 }}>N 值</span>
                            <span style={{ textAlign: "right" }}>
                              <b style={{ color: T.text2 }}>{fmt(base.nRaw)}</b>
                              <span style={{ fontSize: 10.5, color: T.text3 }}> → 封顶后 <b style={{ color: T.text2 }}>{fmt(base.nFinal)}</b></span>
                            </span>
                            <span style={{ textAlign: "right" }}>
                              <b style={{ color: T.ok, fontSize: 13 }}>{fmt(cur.nRaw)}</b>
                              <span style={{ fontSize: 10.5, color: T.text3 }}> → 当前设置下取 <b style={{ color: T.ok }}>{fmt(cur.nFinal)}</b> 条/天</span>
                            </span>
                          </div>
                          <div style={{ marginTop: 8, fontSize: 11, color: T.text3, lineHeight: 1.8 }}>
                            · <b style={{ color: T.text2 }}>N 值理论上限 = {fmt(cur.nRaw)} 条/天</b>（当前窗口下，预算能装下的条数；未受设置封顶）
                            <br />· <b style={{ color: T.text2 }}>N 值当前设置 = {fmt(cur.nFinal)} 条/天</b>（再按「每日上限 {cap} 条/天」封顶后的实际取值）
                            <br />· <b style={{ color: T.text2 }}>快照当时</b>的分配（后端当轮实际执行）：{days > 0 ? `原文保留 ${days} 天` : "未带历史"}
                            {perDay.length ? ` · 每天 ${perDay.join(" / ")} 条` : ""}
                            {dbg.n_used > 0 ? ` · 实发 ${dbg.n_used} 条` : ""}
                            {days > 0 ? " · 更早的天用【前情提要】摘要" : ""}
                            {(() => {
                              // 快照是"当时"的结果：若设置改过（口径/上限/窗口），这里用**当前设置**重算一遍做对照
                              const wouldN = cur.nFinal;
                              const maxPerDay = perDay.length ? Math.max(...perDay.map((x) => Number(x) || 0)) : 0;
                              const changed = wouldN !== maxPerDay;
                              return (
                                <>
                                  <br />· <b style={{ color: T.text2 }}>按当前设置</b>的分配：{days > 0 ? `原文保留 ${days} 天 · 每天最多 ${wouldN} 条` : "未带历史"}
                                  {changed && (
                                    <span style={{ color: T.warn }}>
                                      　⚠ 与快照不同（快照当时每日上限为 {maxPerDay || "—"} 条）：<b>下一条消息</b>才会按 {wouldN} 条/天生效
                                    </span>
                                  )}
                                </>
                              );
                            })()}
                            <br />· <b style={{ color: T.text2 }}>口径提醒</b>：N 是**按消息条数**算的 ——「用户」与「AI」各算 1 条，所以<b>一轮对话 = 2 条 = 2 个 N</b>（例：N=50 条/天 ≈ 25 轮问答/天；想看"几轮"就把 N 除以 2）。
                            <br />· 提示：{cur.nRaw > cap
                              ? `预算还能装 ${fmt(cur.nRaw)} 条/天，但现在被「每日上限」（设置 › API，${cap} 条/天）挡住 —— 想让 AI 看到更多历史就调大它。`
                              : `预算已成瓶颈：调大「每日上限」不会再增加条数，需要调大上下文窗口或降低保留天数。`}
                          </div>
                        </>
                      );
                    })()}
                  </div>
                  <div style={{ fontSize: 11, color: T.text3, lineHeight: 1.8, display: "flex", gap: 7 }}>
                    <span>💡</span><span>裁剪规则：system 提示词（基础指令 / 输出排版 / 数据字典 / 世界观 / 角色 / 方向和关系）与本次输入绝不截断；只截断历史上下文，超限时自动向下调整 N（本次实际带 {dbg.n_used} 条消息，含本次输入）。</span>
                  </div>
                </div>
              ))}
              {currentStory && paramsTab === "preview" && (!dbg ? (
                <div style={{ border: `1px dashed ${T.line}`, borderRadius: 12, padding: "46px 20px", textAlign: "center", color: T.text3, fontSize: 12.5 }}>暂无请求快照，先回剧情区发一条消息。</div>
              ) : (
                <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                    <span style={{ fontSize: 12, color: T.text2 }}>模型 {dbg.model || "—"} · {nodeLabel(dbg.node_id)} · N={dbg.n_used} 条 · 输入 {fmt(usedTokens)} token</span>
                    <button onClick={() => { const full = [dbg.system_prompt, ...history.map((m) => `[${m.role === "user" ? "用户" : "AI"}] ${m.content}`), tailSection ? `[上下文更新]\n${tailSection.text}` : ""].filter(Boolean).join("\n\n"); navigator.clipboard.writeText(full).then(() => setMsg("📋 已复制完整 Prompt")).catch(() => {}); }} style={{ marginLeft: "auto", height: 30, padding: "0 12px", borderRadius: 8, border: `1px solid ${T.line}`, background: T.panel2, color: T.text2, cursor: "pointer", fontSize: 12 }}>📋 复制完整 Prompt</button>
                  </div>
                  <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: "#12141a", overflow: "hidden" }}>
                    {sysSections.map((s, i) => (
                      <div key={`${s.name}-${i}`}>
                        <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "9px 14px", background: T.panel2, borderTop: i ? `1px solid ${T.lineSoft}` : "none", fontSize: 10.5, color: T.text2 }}>
                          <i style={{ width: 8, height: 8, borderRadius: 2, background: sectionColor(s.name) }} />
                          <b>{s.name}</b>
                          <span style={{ color: T.text3 }}>{fmt(estimateTokens(s.text))} token</span>
                        </div>
                        <div style={{ padding: "10px 16px", fontFamily: "monospace", fontSize: 11, lineHeight: 1.9, whiteSpace: "pre-wrap", color: sectionColor(s.name), borderLeft: `2px solid ${sectionColor(s.name)}`, maxHeight: 240, overflowY: "auto" }}>{s.text}</div>
                      </div>
                    ))}
                    <div>
                      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "9px 14px", background: T.panel2, borderTop: `1px solid ${T.lineSoft}`, fontSize: 10.5, color: T.text2 }}>
                        <i style={{ width: 8, height: 8, borderRadius: 2, background: T.accent }} />历史上下文 <span style={{ color: T.text3 }}>{priorHistory.length} 条 · {fmt(histTokens)} token</span>
                      </div>
                      <div style={{ padding: "10px 16px", fontSize: 11, lineHeight: 1.8, maxHeight: 220, overflowY: "auto", borderLeft: `2px solid ${T.accent}`, color: "#c8ccd6", whiteSpace: "pre-wrap" }}>
                        {priorHistory.length === 0 ? "（本次未携带历史）" : priorHistory.map((m, i) => (
                          <div key={i} style={{ marginBottom: 6 }}><span style={{ color: m.role === "user" ? "#ffd7ad" : "#8ab4ff" }}>[{m.role === "user" ? "用户" : "AI"}]</span> {m.content.length > 600 ? m.content.slice(0, 600) + "…" : m.content}</div>
                        ))}
                      </div>
                    </div>
                    {tailSection && (
                      <div>
                        <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "9px 14px", background: T.panel2, borderTop: `1px solid ${T.lineSoft}`, fontSize: 10.5, color: T.text2 }}>
                          <i style={{ width: 8, height: 8, borderRadius: 2, background: "#ff9a45" }} />上下文更新（尾部） <span style={{ color: T.text3 }}>{fmt(estimateTokens(tailSection.text))} token · 每轮单独拼装，不落库</span>
                        </div>
                        <div style={{ padding: "10px 16px", fontFamily: "monospace", fontSize: 11, lineHeight: 1.9, whiteSpace: "pre-wrap", color: "#ffc078", borderLeft: "2px solid #ff9a45", maxHeight: 240, overflowY: "auto" }}>{tailSection.text}</div>
                      </div>
                    )}
                    <div>
                      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "9px 14px", background: T.panel2, borderTop: `1px solid ${T.lineSoft}`, fontSize: 10.5, color: T.text2 }}>
                        <i style={{ width: 8, height: 8, borderRadius: 2, background: "#ffd7ad" }} />本次输入 <span style={{ color: T.text3 }}>{fmt(userTokens)} token</span>
                      </div>
                      <div style={{ padding: "10px 16px", fontFamily: "monospace", fontSize: 11, lineHeight: 1.9, whiteSpace: "pre-wrap", color: "#ffd7ad", borderLeft: "2px solid #ffd7ad" }}>{lastUser?.content || dbg.user_text}</div>
                    </div>
                    <div style={{ display: "flex", gap: 14, flexWrap: "wrap", padding: "10px 14px", borderTop: `1px solid ${T.line}`, background: T.panel2, fontSize: 10.5, color: T.text3 }}>
                      <span>输入 {fmt(usedTokens)}</span>
                      <span>预计输出上限 4,000</span>
                      <span>状态 {dbg.status === "ok" ? "✅ ok" : "❌ " + (dbg.error || "error")}</span>
                      <span style={{ marginLeft: "auto" }}>快照 #{dbg.id} · {dbg.created_at}</span>
                    </div>
                  </div>
                </div>
              ))}
              {currentStory && paramsTab === "json" && (!dbg ? (
                <div style={{ border: `1px dashed ${T.line}`, borderRadius: 12, padding: "46px 20px", textAlign: "center", color: T.text3, fontSize: 12.5 }}>暂无请求快照。</div>
              ) : (() => {
                const jsonText = JSON.stringify({ id: dbg.id, node_id: dbg.node_id, model: dbg.model, estimated_tokens: dbg.estimated_tokens, n_used: dbg.n_used, status: dbg.status, created_at: dbg.created_at, system_prompt: dbg.system_prompt, tail_block: dbg.tail_block, messages: history, ai_reply: dbg.ai_reply }, null, 2);
                return (
                  <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: "#12141a", overflow: "hidden" }}>
                    <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "9px 14px", background: T.panel2, fontSize: 11.5, color: T.text2 }}>
                      <b>本次请求 JSON</b><span style={{ color: T.text3, fontSize: 10.5 }}>system_prompt / tail_block / messages / 元信息</span>
                      <button onClick={() => { navigator.clipboard.writeText(jsonText).then(() => setMsg("📋 已复制 JSON")).catch(() => {}); }} style={{ marginLeft: "auto", height: 28, padding: "0 12px", borderRadius: 7, border: `1px solid ${T.line}`, background: T.panel, color: T.text2, cursor: "pointer", fontSize: 11.5 }}>复制</button>
                    </div>
                    <pre style={{ margin: 0, padding: 14, maxHeight: 460, overflow: "auto", fontFamily: "monospace", fontSize: 11, lineHeight: 1.7, color: "#c8ccd6", whiteSpace: "pre-wrap" }}>{jsonText}</pre>
                  </div>
                );
              })())}
              {currentStory && paramsTab === "image" && (!paramP ? (
                <div style={{ border: `1px dashed ${T.line}`, borderRadius: 12, padding: "46px 20px", textAlign: "center", color: T.text3, fontSize: 12.5 }}>
                  立绘素材库还没有图片。可先到「立绘素材」导入并绑定角色。
                  <button onClick={() => setView("portrait")} style={{ marginLeft: 10, height: 30, padding: "0 12px", borderRadius: 8, border: `1px solid ${T.line}`, background: T.panel2, color: T.text2, cursor: "pointer", fontSize: 12 }}>去立绘素材</button>
                </div>
              ) : (
                <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
                    <span style={{ fontSize: 12, color: T.text3 }}>参数对象</span>
                    <select value={paramP.id} onChange={(e) => setPParamId(Number(e.target.value))} style={{ height: 32, padding: "0 10px", minWidth: 280 }}>
                      {portraits.map((p) => <option key={p.id} value={p.id}>{p.name}{p.bound_character ? ` · ${p.bound_character}` : ""}{p.ver ? ` · ${p.ver}` : ""}</option>)}
                    </select>
                    <span style={{ fontSize: 11, color: T.text3 }}>保存后写入该素材的生成参数（素材详情页同步显示）</span>
                  </div>
                  <div style={{ display: "grid", gridTemplateColumns: "1fr 1.1fr 260px", gap: 14, alignItems: "start" }}>
                    <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, overflow: "hidden" }}>
                      <div style={{ display: "flex", alignItems: "center", gap: 9, padding: "11px 13px", background: T.panel2, borderBottom: `1px solid ${T.lineSoft}` }}>
                        <span style={{ width: 26, height: 26, borderRadius: 8, background: "linear-gradient(135deg,#7b5cff,#3a2b6b)", display: "flex", alignItems: "center", justifyContent: "center", fontSize: 14 }}>🧠</span>
                        <div><b style={{ fontSize: 12.5 }}>模型与采样</b><div style={{ fontSize: 10.5, color: T.text3, marginTop: 2 }}>底模 / LoRA / 采样策略</div></div>
                      </div>
                      <div style={{ padding: 14, display: "flex", flexDirection: "column", gap: 12 }}>
                        <label style={{ fontSize: 11.5, color: T.text2 }}>底模 / 模型名
                          <input value={pParamForm.model} onChange={(e) => setPParamForm({ ...pParamForm, model: e.target.value })} placeholder="例如 sd-xl-1.0-base" style={{ display: "block", width: "100%", marginTop: 6, padding: "7px 10px", boxSizing: "border-box", fontFamily: "monospace", fontSize: 11.5 }} />
                        </label>
                        <div style={{ border: `1px dashed ${T.line}`, borderRadius: 8, padding: "10px 12px", fontSize: 10.5, color: T.text3, lineHeight: 1.7 }}>LoRA / 挂载权重：<b style={{ color: T.text2 }}>占位</b>（依赖插件系统）</div>
                        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10 }}>
                          <label style={{ fontSize: 11.5, color: T.text2 }}>Steps<input value={pParamForm.steps} onChange={(e) => setPParamForm({ ...pParamForm, steps: e.target.value })} placeholder="32" style={{ display: "block", width: "100%", marginTop: 6, padding: "7px 10px", boxSizing: "border-box", fontFamily: "monospace", fontSize: 11.5 }} /></label>
                          <label style={{ fontSize: 11.5, color: T.text2 }}>CFG Scale<input value={pParamForm.cfg} onChange={(e) => setPParamForm({ ...pParamForm, cfg: e.target.value })} placeholder="7.5" style={{ display: "block", width: "100%", marginTop: 6, padding: "7px 10px", boxSizing: "border-box", fontFamily: "monospace", fontSize: 11.5 }} /></label>
                        </div>
                        <label style={{ fontSize: 11.5, color: T.text2 }}>Seed
                          <div style={{ display: "flex", gap: 6, marginTop: 6 }}>
                            <input value={pParamForm.seed} onChange={(e) => setPParamForm({ ...pParamForm, seed: e.target.value })} placeholder="88214" style={{ flex: 1, minWidth: 0, padding: "7px 10px", boxSizing: "border-box", fontFamily: "monospace", fontSize: 11.5 }} />
                            <button onClick={rollParamSeed} title="随机种子" style={{ width: 34, borderRadius: 6, border: `1px solid ${T.line}`, background: T.panel2, cursor: "pointer" }}>🎲</button>
                          </div>
                        </label>
                        <div style={{ border: `1px dashed ${T.line}`, borderRadius: 8, padding: "10px 12px", fontSize: 10.5, color: T.text3 }}>采样器：随生图插件接入（后置）</div>
                      </div>
                    </div>
                    <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, overflow: "hidden" }}>
                      <div style={{ display: "flex", alignItems: "center", gap: 9, padding: "11px 13px", background: T.panel2, borderBottom: `1px solid ${T.lineSoft}` }}>
                        <span style={{ width: 26, height: 26, borderRadius: 8, background: "linear-gradient(135deg,#ff7a18,#ffb066)", display: "flex", alignItems: "center", justifyContent: "center", fontSize: 14 }}>✍️</span>
                        <div><b style={{ fontSize: 12.5 }}>提示词与一致性</b><div style={{ fontSize: 10.5, color: T.text3, marginTop: 2 }}>正面 / 负面 / 参考图锁定</div></div>
                      </div>
                      <div style={{ padding: 14, display: "flex", flexDirection: "column", gap: 12 }}>
                        <label style={{ fontSize: 11.5, color: T.text2 }}>正面提示词
                          <textarea value={pParamForm.prompt} onChange={(e) => setPParamForm({ ...pParamForm, prompt: e.target.value })} rows={4} placeholder="描述角色外观与画面..." style={{ display: "block", width: "100%", marginTop: 6, padding: "8px 10px", boxSizing: "border-box", resize: "vertical", fontFamily: "monospace", fontSize: 11, lineHeight: 1.7, color: "#9fb0c9" }} />
                        </label>
                        <label style={{ fontSize: 11.5, color: T.text2 }}>负面提示词
                          <textarea value={pParamForm.negative} onChange={(e) => setPParamForm({ ...pParamForm, negative: e.target.value })} rows={3} placeholder="blurry, lowres, watermark..." style={{ display: "block", width: "100%", marginTop: 6, padding: "8px 10px", boxSizing: "border-box", resize: "vertical", fontFamily: "monospace", fontSize: 11, lineHeight: 1.7, color: "#ff9d97" }} />
                        </label>
                        <div>
                          <div style={{ fontSize: 11.5, color: T.text2, marginBottom: 6 }}>尺寸与比例</div>
                          <div style={{ display: "flex", gap: 7, flexWrap: "wrap" }}>
                            <span style={{ fontSize: 11, padding: "5px 10px", borderRadius: 7, border: `1px solid ${T.accent}55`, background: T.accentSoft, color: T.accent }}>{paramP.ratio || "未设置"}{paramP.width && paramP.height ? ` · ${paramP.width}×${paramP.height}` : ""}</span>
                            <span style={{ fontSize: 11, padding: "5px 10px", borderRadius: 7, border: `1px solid ${T.line}`, color: T.text3 }}>其他比例（后置）</span>
                          </div>
                        </div>
                        <div style={{ border: `1px solid ${T.line}`, borderRadius: 8, background: "#191c22", padding: "10px 11px" }}>
                          <div style={{ fontSize: 11, color: T.text2, marginBottom: 8 }}>🧩 角色一致性锁定 · 参考已绑定立绘（后置）</div>
                          <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
                            {portraits.filter((p) => p.character_id && p.character_id === paramP.character_id).slice(0, 6).map((p) => (
                              <span key={p.id} style={{ width: 40, height: 40, borderRadius: 8, overflow: "hidden", border: `1px solid ${p.id === paramP.id ? T.accent : T.line}`, background: "#22262f", display: "flex", alignItems: "center", justifyContent: "center" }}>
                                {p.path ? <img src={convertFileSrc(p.path)} alt="" style={{ width: "100%", height: "100%", objectFit: "cover" }} /> : <span>🧑</span>}
                              </span>
                            ))}
                            {portraits.filter((p) => p.character_id && p.character_id === paramP.character_id).length === 0 && <span style={{ fontSize: 10.5, color: T.text3 }}>该素材未绑定角色</span>}
                          </div>
                          <div style={{ fontSize: 10, color: T.text3, marginTop: 8 }}>参考强度 denoise：AI 生成立绘接入后开放</div>
                        </div>
                        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10 }}>
                          <div style={{ border: `1px dashed ${T.line}`, borderRadius: 8, padding: "8px 10px", fontSize: 10.5, color: T.text3 }}>输出格式：PNG · 透明底（后置）</div>
                          <div style={{ border: `1px dashed ${T.line}`, borderRadius: 8, padding: "8px 10px", fontSize: 10.5, color: T.text3 }}>单次张数：4 张 / 批（后置）</div>
                        </div>
                      </div>
                    </div>
                    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                      <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: paramP.path ? "#191c22" : "linear-gradient(160deg,#2a3140,#191c22)", minHeight: 300, display: "flex", alignItems: "center", justifyContent: "center", position: "relative", overflow: "hidden" }}>
                        <span style={{ position: "absolute", top: 9, left: 10, fontSize: 9.5, padding: "2px 7px", borderRadius: 5, background: "rgba(0,0,0,.5)", color: "#dfe3ec" }}>{paramP.ratio || "预览"}{paramP.width && paramP.height ? ` · ${paramP.width}×${paramP.height}` : ""}</span>
                        {paramP.path ? <img src={convertFileSrc(paramP.path)} alt="" style={{ width: "100%", height: "100%", objectFit: "contain", position: "absolute", inset: 0 }} /> : <div style={{ textAlign: "center", color: T.text3 }}><div style={{ fontSize: 40 }}>🧑</div><div style={{ fontSize: 11.5, marginTop: 8 }}>预览区</div><small style={{ fontSize: 9.5 }}>生成后在此显示候选</small></div>}
                        <span style={{ position: "absolute", bottom: 9, left: 10, fontSize: 9.5, color: "#8a91a1", fontFamily: "monospace" }}>seed {pParamForm.seed || "—"} · steps {pParamForm.steps || "—"} · cfg {pParamForm.cfg || "—"}</span>
                      </div>
                      <button onClick={() => setMsg("🎨 AI 生成立绘仍后置（规划中），当前可先保存素材生成参数")} style={{ height: 34, borderRadius: 8, border: "none", background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505", fontWeight: 650, cursor: "pointer", fontSize: 12.5 }}>✨ 生成 4 张（后置）</button>
                      <button onClick={saveParamPortrait} style={{ height: 32, borderRadius: 8, border: `1px solid ${T.line}`, background: T.panel2, color: T.text2, cursor: "pointer", fontSize: 12.5 }}>💾 保存参数</button>
                      <button onClick={() => paramP && setPParamForm({ prompt: paramP.prompt || "", negative: paramP.negative || "", seed: paramP.seed || "", steps: paramP.steps || "", cfg: paramP.cfg || "", model: paramP.model || "" })} style={{ height: 32, borderRadius: 8, border: `1px solid ${T.line}`, background: T.panel2, color: T.text2, cursor: "pointer", fontSize: 12.5 }}>↩ 恢复当前值</button>
                      <div style={{ fontSize: 10.5, color: T.text3, lineHeight: 1.8, padding: "2px 4px" }}>
                        素材：{paramP.name} · {paramP.ver || "v1"}<br />
                        绑定角色：{paramP.bound_character || paramP.role || "未绑定"}<br />
                        更新：{paramP.date || "—"}
                      </div>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          );
        })()}
        {view === "plugins" && (
          <div style={{ maxWidth: 720 }}>
            <h3 style={{ margin: 0, marginBottom: 6 }}>🧩 插件 / 扩展</h3>
            <p style={{ fontSize: 12.5, color: T.text3, marginBottom: 14 }}>
              核心功能保持精简，扩展功能以插件形式接入（插件系统后置）。以下为规划的插件位，随插件系统一并实装。
            </p>
            <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
              {PLUGIN_PLACEHOLDERS.map((p) => (
                <div key={p.name} style={{ display: "flex", alignItems: "flex-start", gap: 12, border: `1px solid ${T.line}`, borderRadius: 12, padding: "14px 16px", background: T.panel }}>
                  <span style={{ fontSize: 22 }}>{p.icon}</span>
                  <div style={{ flex: 1 }}>
                    <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                      <b style={{ fontSize: 13.5 }}>{p.name}</b>
                      <span style={{ fontSize: 10, padding: "1px 8px", borderRadius: 9, border: `1px dashed ${T.line}`, color: T.text3 }}>{p.status}</span>
                    </div>
                    <div style={{ fontSize: 12, color: T.text3, marginTop: 4, lineHeight: 1.5 }}>{p.desc}</div>
                  </div>
                  <button disabled style={{ fontSize: 12, padding: "4px 12px", cursor: "not-allowed", opacity: 0.5 }}>未开放</button>
                </div>
              ))}
            </div>
            <p style={{ fontSize: 11.5, color: T.text3, marginTop: 14 }}>插件系统目标：应对第三方服务下线风险、保持软件轻量化；Ollama 本地模型以插件形式接入。</p>
          </div>
        )}

        {view === "story" && (
          <>
            <div style={{ display: "flex", gap: 8, alignItems: "center", marginBottom: 16, overflowX: "auto", paddingBottom: 4 }}>
              <span style={{ fontSize: 12, color: T.text3, flexShrink: 0 }}>时间线：</span>
              {timelineNodes.map((n) => {
                const nd = nodeDateDisplay(n);
                return (
                <span key={n.id} style={{ display: "inline-flex", alignItems: "center", flexShrink: 0 }}>
                  <button
                    onClick={() => switchNode(n.id)}
                    style={{
                      padding: "4px 10px", fontSize: 13, borderRadius: "14px 0 0 14px",
                      border: `1px solid ${T.line}`, borderRight: "none",
                      background: node?.id === n.id ? T.accentSoft : T.panel2,
                      color: node?.id === n.id ? T.accent : T.text2,
                      cursor: "pointer", fontStyle: n.node_type === "interlude" ? "italic" : "normal",
                    }}
                  >
                    {n.node_type === "interlude" ? "🎭 " : ""}{n.date_label}{nd && <span style={{ marginLeft: 4, fontSize: 10, color: T.text3, fontWeight: 400 }}> · {nd}</span>}{n.tags ? <span style={{ marginLeft: 4, fontSize: 9, color: T.accent }}>●</span> : null}{n.day_summary ? <span title={"已生成日摘要：" + n.day_summary} style={{ marginLeft: 4, fontSize: 9, color: T.ok }}>📝</span> : null}
                  </button>
                  <button
                    onClick={() => removeNode(n.id, n.date_label)}
                    title="删除该节点"
                    style={{
                      padding: "4px 8px", fontSize: 13, borderRadius: "0 14px 14px 0",
                      border: `1px solid ${T.line}`,
                      background: node?.id === n.id ? T.accentSoft : T.panel2,
                      color: node?.id === n.id ? T.accent : T.danger, cursor: "pointer",
                    }}
                  >
                    ×
                  </button>
                </span>
                );
              })}
              <button onClick={addNextDay} style={{ padding: "4px 12px", fontSize: 13, borderRadius: 14, border: `1px dashed ${T.text3}`, background: "transparent", color: T.text2, cursor: "pointer", flexShrink: 0 }}>+ 下一天</button>
              <button onClick={addInterlude} style={{ padding: "4px 12px", fontSize: 13, borderRadius: 14, border: `1px dashed ${T.danger}`, background: "transparent", color: "#c98a80", cursor: "pointer", flexShrink: 0 }}>🎭 插入插曲</button>
            </div>
            {/* 这一天相关的操作（不跟时间线按钮挤在同一行，避免横向滚动时看不见） */}
            <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 14, flexWrap: "wrap" }}>
              <span style={{ fontSize: 11.5, color: T.text3 }}>本日：</span>
              <button
                onClick={() => { const cur = timelineNodes.find((n) => n.id === node?.id); if (cur?.day_summary) { if (confirm("这天已有日摘要，重新生成会覆盖它。\n（摘要一旦生成就不再回改，覆盖后与历史对话的一致性由你把握）\n\n确定覆盖？")) makeDaySummary(true); } else makeDaySummary(false); }}
                title="为当前这天生成日摘要（供后续跨天回读；生成一次即固化）"
                style={{ padding: "3px 11px", fontSize: 12, borderRadius: 8, border: `1px solid ${T.ok}`, background: "rgba(55,201,120,.08)", color: T.ok, cursor: "pointer" }}
              >📝 生成日摘要</button>
              <span style={{ fontSize: 10.5, color: T.text3 }}>
                {(() => { const cur = timelineNodes.find((n) => n.id === node?.id); return cur?.day_summary ? "这天已有摘要（📝 已固化，不会自动回改）" : "这天还没有摘要 —— 翻篇后会自动补"; })()}
              </span>
            </div>

            <div style={{ display: "flex", flexDirection: "column", gap: 18, minHeight: 200 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 10, color: T.text3, fontSize: 11 }}>
                <span style={{ flex: 1, height: 1, background: T.lineSoft }} />
                {node ? node.date_label : "—"} · {messages.length} 条消息
                <span style={{ flex: 1, height: 1, background: T.lineSoft }} />
              </div>
              {/* D 子项：当前这天的日摘要（生成一次即固化；没有就提示可生成） */}
              {(() => {
                const cur = timelineNodes.find((n) => n.id === node?.id);
                if (!cur) return null;
                return (
                  <div style={{ border: `1px ${cur.day_summary ? "solid" : "dashed"} ${cur.day_summary ? "rgba(55,201,120,.35)" : T.lineSoft}`, borderRadius: 10, background: cur.day_summary ? "rgba(55,201,120,.05)" : "transparent", padding: "10px 14px" }}>
                    <div style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 11.5, color: cur.day_summary ? T.ok : T.text3, marginBottom: cur.day_summary ? 6 : 0, flexWrap: "wrap" }}>
                      <b>📝 日摘要（这天发生了什么）</b>
                      {cur.day_summary
                        ? <span style={{ color: T.text3, fontWeight: 400 }}>已固化，不会自动回改；点右侧可覆盖</span>
                        : <span style={{ fontWeight: 400 }}>还没生成 —— 翻篇后会自动补；跨天回读时更早的日子只带摘要</span>}
                      <button
                        onClick={() => { if (cur.day_summary) { if (confirm("这天已有日摘要，重新生成会覆盖它。\n（摘要一旦生成就不再回改，覆盖后与历史对话的一致性由你把握）\n\n确定覆盖？")) makeDaySummary(true); } else makeDaySummary(false); }}
                        style={{ marginLeft: "auto", padding: "2px 10px", fontSize: 11, borderRadius: 7, border: `1px solid ${T.ok}`, background: "rgba(55,201,120,.08)", color: T.ok, cursor: "pointer" }}
                      >{cur.day_summary ? "重新生成" : "📝 生成日摘要"}</button>
                    </div>
                    {cur.day_summary && <div style={{ fontSize: 12, color: T.text2, lineHeight: 1.75 }}>{cur.day_summary}</div>}
                  </div>
                );
              })()}
              {messages.length === 0 && (
                <p style={{ color: T.text3, fontSize: 13 }}>这一天还没有剧情。在下面输入开始。</p>
              )}
              {messages.map((m) => (
                <div key={m.id} style={{ display: "flex", gap: 10, maxWidth: "78%", alignSelf: m.role === "user" ? "flex-end" : "flex-start", flexDirection: m.role === "user" ? "row-reverse" : "row" }}>
                  <div style={{ width: 30, height: 30, flex: "0 0 30px", borderRadius: 9, marginTop: 2, display: "flex", alignItems: "center", justifyContent: "center", fontSize: 11, fontWeight: 600, background: m.role === "user" ? "linear-gradient(135deg,#5b8cff,#8f6bff)" : "linear-gradient(135deg,#FF7A18,#ffc078)", color: m.role === "user" ? "#fff" : "#231505" }}>
                    {m.role === "user" ? "我" : "✦"}
                  </div>
                  <div style={{ minWidth: 0 }}>
                    <div style={{ fontSize: 11, color: T.text3, marginBottom: 5, display: "flex", gap: 8, alignItems: "center", justifyContent: m.role === "user" ? "flex-end" : "flex-start" }}>
                      <span style={{ color: T.text2, fontWeight: 600 }}>{m.role === "user" ? "我" : "导演模式"}</span>
                      <span>{m.created_at}</span>
                      {editingId !== m.id && (
                        <button onClick={() => startEdit(m)} style={{ fontSize: 11, background: "none", border: "none", color: T.text3, cursor: "pointer", padding: 0 }}>✏️ 编辑</button>
                      )}
                    </div>
                    {editingId === m.id ? (
                      <div>
                        <textarea value={editText} onChange={(e) => setEditText(e.target.value)} style={{ width: "100%", minHeight: 80, padding: 6, fontSize: 13, fontFamily: "inherit", borderRadius: 4, border: `1px solid ${T.line}`, background: T.panel2, color: T.text1 }} />
                        <div style={{ marginTop: 6, display: "flex", gap: 8 }}>
                          <button onClick={saveEdit} style={{ fontSize: 12, padding: "3px 10px", cursor: "pointer", border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, borderRadius: 6 }}>保存</button>
                          <button onClick={cancelEdit} style={{ fontSize: 12, padding: "3px 10px", cursor: "pointer", border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, borderRadius: 6 }}>取消</button>
                        </div>
                      </div>
                    ) : (
                      <>
                        {/* 导演指令：独立小条显示在气泡上方，正文里不重复出现 */}
                        {m.role === "user" && parseDirector(m.content) && (
                          <div style={{ marginBottom: 5, padding: "5px 11px", borderRadius: 9, border: `1px dashed rgba(255,122,24,.45)`, background: T.accentSoft, color: T.accent, fontSize: 11, lineHeight: 1.6, whiteSpace: "pre-wrap", textAlign: "left" }}>
                            🎬 导演指令：{parseDirector(m.content)}
                          </div>
                        )}
                        {(m.role !== "user" || stripDirector(m.content)) && (
                      <div style={{ padding: "11px 14px", borderRadius: 14, fontSize: 13.5, lineHeight: 1.75, wordBreak: "break-word", position: "relative", background: m.role === "user" ? "linear-gradient(135deg,#3573ee,#2b5fd0)" : T.aiBubble, border: m.role === "user" ? "none" : `1px solid ${T.lineSoft}`, borderTopLeftRadius: m.role === "user" ? 14 : 5, borderTopRightRadius: m.role === "user" ? 5 : 14, color: m.role === "user" ? "#fff" : "#dfe3ec", boxShadow: m.role === "user" ? "0 4px 14px rgba(47,111,235,.24)" : "none" }}>
                        {m.role === "user" ? (
                        <span style={{ whiteSpace: "pre-wrap" }}>{stripDirector(m.content)}</span>
                      ) : (
                        <div>
                          {parseScriptBlocks(m.content, chars.map((cc) => cc.name)).map((b, bi) =>
                            b.type === "speaker" ? (
                              <div key={bi} style={{ fontSize: 12, fontWeight: 700, color: T.accent, margin: "10px 0 2px", letterSpacing: 1 }}>{b.text}</div>
                            ) : b.type === "narr" ? (
                              <div key={bi} style={{ color: "#9aa3b2", lineHeight: 1.8, marginBottom: 8 }}>{b.text}</div>
                            ) : b.type === "action" ? (
                              <div key={bi} style={{ color: "#8a91a1", fontStyle: "italic", lineHeight: 1.8, marginBottom: 8 }}>{b.text}</div>
                            ) : b.type === "inner" ? (
                              /* 沉浸式心声（角色内心原声）：斜体 + 暖玫瑰色 + 左侧竖线，与旁白/动作区分 */
                              <div key={bi} style={{ color: "#d9a7bd", fontStyle: "italic", lineHeight: 1.8, marginBottom: 8, paddingLeft: 10, borderLeft: "2px solid rgba(217,167,189,.45)" }}>{b.text}</div>
                            ) : (
                              <div key={bi} style={{ marginBottom: 8, lineHeight: 1.8 }}>
                                <span style={{ display: "inline-block", fontSize: 11, fontWeight: 600, color: T.accent, background: T.accentSoft, padding: "1px 8px", borderRadius: 9, marginRight: 8, verticalAlign: 1 }}>{b.name}</span>
                                <span style={{ color: "#dfe3ec" }}>{b.text}</span>
                              </div>
                            )
                          )}
                        </div>
                      )}
                        {m.id < 0 && (
                          <span style={{ display: "inline-block", width: 7, height: 15, background: T.accent, verticalAlign: -2, marginLeft: 2, animation: "dsh-blink 1s steps(2) infinite", borderRadius: 1 }} />
                        )}
                      </div>
                        )}
                      </>
                    )}
                  </div>
                </div>
              ))}
            </div>

          </>
        )}
          </div>

          {/* 底部输入台（仅剧情 view） */}
          {view === "story" && (
            <div style={{ borderTop: `1px solid ${T.line}`, background: T.panel, padding: "12px 24px 14px", flexShrink: 0 }}>
              <div style={{ border: `1px solid ${T.line}`, background: T.panel2, borderRadius: 12, paddingBottom: 4 }}>
                <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 12px 0", flexWrap: "wrap" }}>
                  <div style={{ display: "inline-flex", background: "#191c22", border: `1px solid ${T.line}`, borderRadius: 8, padding: 2 }}>
                    <button onClick={() => setNavMode("旁白")} style={{ border: "none", background: "transparent", color: navMode === "旁白" ? T.accent : T.text3, fontSize: 11.5, padding: "4px 11px", borderRadius: 6, cursor: "pointer", fontWeight: navMode === "旁白" ? 600 : 400, fontFamily: "inherit" }}>🎙 旁白</button>
                    <button onClick={() => setNavMode("对话")} style={{ border: "none", background: "transparent", color: navMode === "对话" ? T.accent : T.text3, fontSize: 11.5, padding: "4px 11px", borderRadius: 6, cursor: "pointer", fontWeight: navMode === "对话" ? 600 : 400, fontFamily: "inherit" }}>💬 对话</button>
                  </div>
                  {navMode === "对话" && (
                    <div style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
                      <span style={{ fontSize: 11, color: T.text3 }}>我扮演</span>
                      <select value={myRole} onChange={(e) => setMyRole(e.target.value)} style={{ background: "#191c22", border: `1px solid ${T.line}`, color: T.text1, borderRadius: 6, fontSize: 11.5, padding: "3px 6px", outline: "none", fontFamily: "inherit" }}>
                        <option value="自由">自由</option>
                        {chars.map((cc) => <option key={cc.id} value={cc.name}>{cc.name}</option>)}
                      </select>
                      <span style={{ fontSize: 11, color: T.text3 }}>AI 扮演</span>
                      <select value={aiRole} onChange={(e) => setAiRole(e.target.value)} style={{ background: "#191c22", border: `1px solid ${T.line}`, color: T.text1, borderRadius: 6, fontSize: 11.5, padding: "3px 6px", outline: "none", fontFamily: "inherit" }}>
                        <option value="自由">自由</option>
                        {chars.map((cc) => <option key={cc.id} value={cc.name}>{cc.name}</option>)}
                      </select>
                    </div>
                  )}
                  <span style={{ position: "relative" }}>
                    <span onClick={() => setShowAt((v) => !v)} title="绑定本次聚焦角色" style={{ fontSize: 11, color: showAt ? T.accent : T.text3, border: showAt ? `1px solid ${T.accent}` : `1px solid ${T.line}`, borderRadius: 6, padding: "3px 8px", cursor: "pointer", background: showAt ? T.accentSoft : "transparent" }}><span style={{ color: T.accent }}>@</span> 角色</span>
                    {showAt && (
                      <div style={{ position: "absolute", top: "100%", left: 0, zIndex: 30, marginTop: 4, background: T.panel2, border: `1px solid ${T.line}`, borderRadius: 8, padding: 8, minWidth: 150, boxShadow: "0 10px 28px rgba(0,0,0,.45)", display: "flex", flexWrap: "wrap", gap: 6 }}>
                        {chars.length === 0 && <div style={{ fontSize: 11, color: T.text3, padding: 4 }}>暂无角色，先到角色管理创建</div>}
                        {chars.map((cc) => (
                          <span key={cc.id} onClick={() => { setInput((v) => (v ? v + " " : "") + "@" + cc.name + " "); setShowAt(false); }} style={{ fontSize: 11.5, color: T.text1, background: "#191c22", border: `1px solid ${T.lineSoft}`, borderRadius: 6, padding: "3px 9px", cursor: "pointer" }}>{cc.name}</span>
                        ))}
                      </div>
                    )}
                  </span>
                  <span style={{ position: "relative" }}>
                    <span onClick={() => setShowTag((v) => !v)} title="给本节点打标签" style={{ fontSize: 11, color: showTag ? T.accent : T.text3, border: showTag ? `1px solid ${T.accent}` : `1px solid ${T.line}`, borderRadius: 6, padding: "3px 8px", cursor: "pointer", background: showTag ? T.accentSoft : "transparent" }}><span style={{ color: T.accent }}>#</span> 标签</span>
                    {showTag && (
                      <div style={{ position: "absolute", top: "100%", left: 0, zIndex: 30, marginTop: 4, background: T.panel2, border: `1px solid ${T.line}`, borderRadius: 8, padding: 8, minWidth: 210, boxShadow: "0 10px 28px rgba(0,0,0,.45)", display: "flex", flexWrap: "wrap", gap: 6, alignItems: "center" }}>
                        {PRESET_TAGS.map((t) => (
                          <span key={t} onClick={() => { setInput((v) => (v ? v + " " : "") + "#" + t + " "); setShowTag(false); }} style={{ fontSize: 11.5, color: T.text1, background: "#191c22", border: `1px solid ${T.lineSoft}`, borderRadius: 6, padding: "3px 9px", cursor: "pointer" }}>#{t}</span>
                        ))}
                        <input
                          value={tagCustom}
                          onChange={(e) => setTagCustom(e.target.value)}
                          onKeyDown={(e) => { if (e.key === "Enter" && tagCustom.trim()) { setInput((v) => (v ? v + " " : "") + "#" + tagCustom.trim() + " "); setTagCustom(""); setShowTag(false); } }}
                          placeholder="自定义标签"
                          style={{ width: 90, fontSize: 11.5, background: "#191c22", border: `1px solid ${T.line}`, color: T.text1, borderRadius: 6, padding: "3px 8px", outline: "none" }}
                        />
                      </div>
                    )}
                  </span>
                  <span style={{ marginLeft: "auto", fontSize: 11, color: T.text3 }}>旁白＝上帝扩写 · 对话＝沉浸扮演</span>
                </div>
                <textarea
                  value={input}
                  onChange={(e) => setInput(e.target.value)}
                  onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); sendMessage(); } }}
                  placeholder="描述你想要的剧情走向、语气或镜头感…（Shift+Enter 换行）"
                  style={{ width: "100%", boxSizing: "border-box", border: "none", background: "transparent", resize: "none", outline: "none", color: T.text1, fontSize: 13.5, lineHeight: 1.7, padding: "10px 14px 4px", minHeight: 56, fontFamily: "inherit" }}
                />
                <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "6px 12px 10px", flexWrap: "wrap" }}>
                  <button onClick={() => setSensitiveFilter(!sensitiveFilter)} style={{ display: "inline-flex", alignItems: "center", gap: 6, height: 28, padding: "0 10px", borderRadius: 7, border: `1px solid ${T.line}`, background: "#191c22", color: sensitiveFilter ? T.ok : T.text2, fontSize: 11.5, cursor: "pointer", fontFamily: "inherit" }}>🛡 敏感词过滤</button>
                  <button onClick={() => setShowConstraintInput(!showConstraintInput)} style={{ display: "inline-flex", alignItems: "center", gap: 6, height: 28, padding: "0 10px", borderRadius: 7, border: `1px solid ${T.line}`, background: "#191c22", color: constraints.length > 0 || showConstraintInput ? T.ok : T.text2, fontSize: 11.5, cursor: "pointer", fontFamily: "inherit" }}>🔒 剧情约束{constraints.length > 0 ? `(${constraints.length})` : ""}</button>
                  {navMode === "对话" && (
                    <button onClick={() => setShowDirectorInput(!showDirectorInput)} title="导演指令：强制剧情这样走（一次性，附加到本次发送）" style={{ display: "inline-flex", alignItems: "center", gap: 6, height: 28, padding: "0 10px", borderRadius: 7, border: `1px solid ${directors.length > 0 ? "rgba(255,122,24,.42)" : T.line}`, background: directors.length > 0 ? T.accentSoft : "#191c22", color: directors.length > 0 || showDirectorInput ? T.accent : T.text2, fontSize: 11.5, cursor: "pointer", fontFamily: "inherit" }}>🎬 导演指令{directors.length > 0 ? `(${directors.length})` : ""}</button>
                  )}
                  <span style={{ display: "inline-flex", alignItems: "center", gap: 6, height: 28, padding: "0 10px", borderRadius: 7, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, fontSize: 11.5, fontFamily: "inherit" }}>
                    📏 长度
                    <span style={{ display: "inline-flex", gap: 2 }}>
                      {([["简单", "简单描写：篇幅精简，五到十行，动作神态择要，心理不省，环境不展开"], ["详细", "详细展开：篇幅不限，动作/神态/心理/环境四项充分描写"]] as const).map(([k, tip]) => (
                        <button
                          key={k}
                          onClick={() => setLenLevel(k)}
                          title={tip}
                          style={{ height: 20, padding: "0 9px", borderRadius: 5, border: "none", cursor: "pointer", fontFamily: "inherit", fontSize: 11, background: lenLevel === k ? T.accentSoft : "transparent", color: lenLevel === k ? T.accent : T.text3, fontWeight: lenLevel === k ? 600 : 400 }}
                        >{k}</button>
                      ))}
                    </span>
                  </span>
                  <span onClick={() => setMemoryCtx(!memoryCtx)} style={{ display: "inline-flex", alignItems: "center", gap: 7, fontSize: 11.5, color: T.text2, cursor: "pointer" }}>
                    <span style={{ width: 30, height: 17, borderRadius: 9, background: memoryCtx ? "rgba(55,201,120,.6)" : T.line, position: "relative", display: "inline-block", transition: "background .2s" }}>
                      <span style={{ position: "absolute", top: 2, left: memoryCtx ? 15 : 2, width: 13, height: 13, borderRadius: "50%", background: memoryCtx ? "#1c1f26" : "#22262f", transition: "left .2s" }} />
                    </span>记忆上下文
                  </span>
                  {showConstraintInput && (
                    <div style={{ width: "100%", display: "flex", flexDirection: "column", gap: 6, marginTop: 2 }}>
                      <div style={{ display: "flex", gap: 6 }}>
                        <input
                          value={constraintDraft}
                          onChange={(e) => setConstraintDraft(e.target.value)}
                          onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); const v = constraintDraft.trim(); if (v) { updateConstraints([...constraints, v]); setConstraintDraft(""); } } }}
                          placeholder="添加剧情约束，如：角色不会死亡（回车添加）"
                          style={{ flex: 1, padding: "7px 10px", borderRadius: 7, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontSize: 12, outline: "none", fontFamily: "inherit" }}
                        />
                        <button onClick={() => { const v = constraintDraft.trim(); if (v) { updateConstraints([...constraints, v]); setConstraintDraft(""); } }} style={{ padding: "0 12px", border: "none", borderRadius: 7, background: T.accent, color: "#231505", fontSize: 12, fontWeight: 600, cursor: "pointer", fontFamily: "inherit" }}>添加</button>
                      </div>
                      {constraints.length > 0 && (
                        <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
                          {constraints.map((cst, i) => (
                            <span key={i} style={{ display: "inline-flex", alignItems: "center", gap: 5, padding: "3px 8px", borderRadius: 10, border: `1px solid ${T.line}`, background: T.panel2, color: T.text2, fontSize: 11 }}>
                              {cst}
                              <button onClick={() => updateConstraints(constraints.filter((_, j) => j !== i))} style={{ color: T.danger, background: "none", border: "none", cursor: "pointer", fontSize: 11, padding: 0, fontFamily: "inherit" }}>✕</button>
                            </span>
                          ))}
                        </div>
                      )}
                    </div>
                  )}
                  {showDirectorInput && navMode === "对话" && (
                    <div style={{ width: "100%", display: "flex", flexDirection: "column", gap: 6, marginTop: 2 }}>
                      <div style={{ display: "flex", gap: 6 }}>
                        <input
                          value={directorDraft}
                          onChange={(e) => setDirectorDraft(e.target.value)}
                          onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); const v = directorDraft.trim(); if (v) { updateDirectors([...directors, v]); setDirectorDraft(""); } } }}
                          placeholder="导演指令：强制剧情这样走，如：强制让夏婉琴接受告白（回车添加，发送后自动清空）"
                          style={{ flex: 1, padding: "7px 10px", borderRadius: 7, border: `1px solid rgba(255,122,24,.35)`, background: "#191c22", color: T.text1, fontSize: 12, outline: "none", fontFamily: "inherit" }}
                        />
                        <button onClick={() => { const v = directorDraft.trim(); if (v) { updateDirectors([...directors, v]); setDirectorDraft(""); } }} style={{ padding: "0 12px", border: "none", borderRadius: 7, background: T.accent, color: "#231505", fontSize: 12, fontWeight: 600, cursor: "pointer", fontFamily: "inherit" }}>添加</button>
                      </div>
                      {directors.length > 0 && (
                        <div style={{ display: "flex", flexWrap: "wrap", gap: 6, alignItems: "center" }}>
                          {directors.map((d, i) => (
                            <span key={i} title={d} style={{ display: "inline-flex", alignItems: "center", gap: 5, maxWidth: 320, padding: "3px 8px", borderRadius: 10, border: `1px solid rgba(255,122,24,.42)`, background: T.accentSoft, color: T.accent, fontSize: 11 }}>
                              <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>🎬 {d}</span>
                              <button onClick={() => updateDirectors(directors.filter((_, j) => j !== i))} style={{ color: T.danger, background: "none", border: "none", cursor: "pointer", fontSize: 11, padding: 0, fontFamily: "inherit" }}>✕</button>
                            </span>
                          ))}
                          <span style={{ fontSize: 10.5, color: T.text3 }}>这些指令只在下次发送时生效，发完自动清空</span>
                        </div>
                      )}
                    </div>
                  )}
                  <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 10 }}>
                    <span title={dayDebug ? `当前节点 ${node?.date_label ?? "—"} · 已用 ${dayBudget.usedTokens.toLocaleString("en-US")} / ${dayBudget.inputBudget.toLocaleString("en-US")} token · 剩余 ${dayBudget.remain.toLocaleString("en-US")}` : "当前节点还没有请求快照"} style={{ display: "inline-flex", alignItems: "center", gap: 6, fontSize: 10.5, color: T.text3, cursor: "default" }}>
                      <span style={{ width: 7, height: 7, borderRadius: "50%", background: dayDebug ? (dayBudget.pctUsed > 85 ? T.danger : dayBudget.pctUsed > 60 ? T.warn : T.ok) : T.line }} />
                      <span>Token {dayDebug ? `${dayBudget.pctUsed}%` : "—"}</span>
                      <span style={{ width: 46, height: 5, borderRadius: 3, background: "#12141a", border: `1px solid ${T.lineSoft}`, overflow: "hidden", display: "inline-block" }}>
                        <span style={{ display: "block", height: "100%", width: `${dayBudget.pctUsed}%`, background: dayDebug ? (dayBudget.pctUsed > 85 ? T.danger : dayBudget.pctUsed > 60 ? T.warn : T.ok) : "transparent" }} />
                      </span>
                    </span>
                    <span style={{ fontSize: 10.5, color: T.text3 }}>{input.length} / 2000 · <span style={{ background: "#191c22", border: `1px solid ${T.line}`, borderBottomWidth: 2, borderRadius: 4, padding: "0 4px", fontSize: 10, color: T.text2 }}>Enter</span> 发送</span>
                    <button onClick={sendMessage} style={{ height: 32, padding: "0 18px", border: "none", borderRadius: 8, cursor: "pointer", background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505", fontSize: 13, fontWeight: 650, fontFamily: "inherit", boxShadow: "0 4px 14px rgba(255,122,24,.26)" }}>➤ 提交</button>
                  </div>
                </div>
              </div>
            </div>
          )}
        </div>

        {/* ③ 右侧信息面板（导演监视器 / 素材库） */}
        <div style={{ width: 300, borderLeft: `1px solid ${T.line}`, background: T.panel, flexShrink: 0, display: "flex", flexDirection: "column", minHeight: 0 }}>
          <div style={{ height: 52, flexShrink: 0, borderBottom: `1px solid ${T.line}`, display: "flex", alignItems: "center", gap: 8, padding: "0 16px" }}>
            <span style={{ color: T.accent, fontSize: 16 }}>🎬</span>
            <span style={{ fontSize: 13, fontWeight: 600 }}>导演监视器</span>
            <span style={{ marginLeft: "auto", color: T.text3, fontSize: 12, cursor: "pointer" }} title="面板设置（占位）">⚙</span>
          </div>
          <div style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: "14px 16px 18px", display: "flex", flexDirection: "column", gap: 20 }}>
            {/* 角色立绘（占位卡片） */}
            <div>
              <div style={{ fontSize: 10.5, color: T.text3, letterSpacing: 1.1, marginBottom: 10, display: "flex", alignItems: "center", gap: 6 }}>
                <span>🖼 角色立绘</span>
                <span style={{ marginLeft: "auto" }}>{chars.length} 位角色</span>
              </div>
              <div style={{ display: "flex", gap: 10, overflowX: "auto", paddingBottom: 6 }}>
                {chars.length === 0 && (
                  <div style={{ width: 86, flex: "0 0 86px", borderRadius: 12, border: `1px dashed ${T.line}`, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 4, color: T.text3, fontSize: 11, padding: "18px 0" }}>
                    <span style={{ fontSize: 20 }}>🎭</span>暂无角色
                  </div>
                )}
                {chars.map((ch) => {
                  const emo = emotionInfo(ch.current_status || "");
                  const f = emo.label ? emo : null;
                  const focusNow = focusCharId ?? chars.find((x) => x.current_status)?.id ?? chars[0]?.id ?? null;
                  const active = focusNow === ch.id;
                  const bound = portraits.filter((p) => p.character_id === ch.id && p.story_id === currentStory?.id);
                  if (bound.length === 0) {
                    return (
                      <div key={ch.id} title="点击设为焦点角色（未绑定立绘，可在「立绘素材」中导入并绑定）" onClick={() => setFocusCharId(ch.id)} style={{ width: 86, flex: "0 0 86px", borderRadius: 12, overflow: "hidden", border: `1px solid ${active ? T.accent : f ? emo.color : T.line}`, background: T.panel2, cursor: "pointer", position: "relative", boxShadow: active ? `0 0 10px ${T.accent}55` : f ? `0 0 12px ${emo.color}66` : "none", transition: "box-shadow .3s" }}>
                        <div style={{ height: 104, display: "flex", alignItems: "center", justifyContent: "center", background: f ? `linear-gradient(160deg, ${emo.color}88, #22293a)` : "linear-gradient(160deg,#4a5f8a,#22293a)" }}>
                          <span style={{ fontSize: 34, color: "rgba(255,255,255,.72)" }}>🧑</span>
                        </div>
                        <div style={{ padding: "6px 7px", fontSize: 11, textAlign: "center", color: T.text2 }}>{ch.name}</div>
                        {f && <span style={{ position: "absolute", top: 5, left: 5, fontSize: 9, padding: "1px 6px", borderRadius: 4, background: emo.color, color: "#fff", fontWeight: 600 }}>{emo.label}</span>}
                        {active ? <span style={{ position: "absolute", top: 5, right: 5, fontSize: 9, padding: "1px 5px", borderRadius: 4, background: T.accent, color: "#231505", fontWeight: 700 }}>焦点</span> : <span style={{ position: "absolute", top: 5, right: 5, fontSize: 9, padding: "1px 5px", borderRadius: 4, background: "rgba(0,0,0,.55)", color: "#ffd7ad" }}>占位</span>}
                      </div>
                    );
                  }
                  return bound.map((p) => (
                    <div key={p.id} title="点击设为焦点角色" onClick={() => setFocusCharId(ch.id)} style={{ width: 86, flex: "0 0 86px", borderRadius: 12, overflow: "hidden", border: `1px solid ${active ? T.accent : f ? emo.color : T.line}`, background: T.panel2, cursor: "pointer", position: "relative", boxShadow: active ? `0 0 10px ${T.accent}55` : f ? `0 0 12px ${emo.color}66` : "none", transition: "box-shadow .3s" }}>
                      <div style={{ height: 104, display: "flex", alignItems: "center", justifyContent: "center", background: f ? `linear-gradient(160deg, ${emo.color}88, #22293a)` : "linear-gradient(160deg,#4a5f8a,#22293a)" }}>
                        {p.path ? <img src={convertFileSrc(p.path)} alt="" style={{ width: "100%", height: "100%", objectFit: "cover", display: "block" }} /> : <span style={{ fontSize: 34, color: "rgba(255,255,255,.72)" }}>🧑</span>}
                      </div>
                      <div style={{ padding: "6px 7px", fontSize: 11, textAlign: "center", color: T.text2 }}>{ch.name}</div>
                      {f && <span style={{ position: "absolute", top: 5, left: 5, fontSize: 9, padding: "1px 6px", borderRadius: 4, background: emo.color, color: "#fff", fontWeight: 600 }}>{emo.label}</span>}
                      {p.role && <span style={{ position: "absolute", top: 5, right: 5, fontSize: 9, padding: "1px 5px", borderRadius: 4, background: "rgba(0,0,0,.55)", color: T.accent }}>{p.role}</span>}
                      <span onClick={(e) => { e.stopPropagation(); setView("portrait"); setPDetail(p.id); }} title="查看素材详情" style={{ position: "absolute", right: 4, bottom: 24, fontSize: 9, padding: "1px 5px", borderRadius: 4, background: "rgba(0,0,0,.6)", color: T.accent, cursor: "pointer" }}>⛶</span>
                    </div>
                  ));
                })}
                <div title="去「立绘素材」导入并绑定角色" onClick={() => setView("portrait")} style={{ width: 86, flex: "0 0 86px", borderRadius: 12, border: `1px dashed ${T.line}`, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 4, color: T.text3, fontSize: 11, cursor: "pointer", padding: "18px 0" }}>
                  <span style={{ fontSize: 20 }}>＋</span>添加
                </div>
              </div>
            </div>

            {/* 当前角色卡（焦点角色详情） */}
            <div>
              <div style={{ fontSize: 10.5, color: T.text3, letterSpacing: 1.1, marginBottom: 10, display: "flex", alignItems: "center", gap: 6 }}>
                <span>🪪 当前角色卡</span>
                <span style={{ marginLeft: "auto" }}>焦点角色 · 点击右侧立绘切换</span>
              </div>
              {(() => {
                const focusCh = chars.find((c) => c.id === focusCharId) ?? chars.find((x) => x.current_status) ?? chars[0] ?? null;
                if (!focusCh) {
                  return (
                    <div style={{ border: `1px dashed ${T.line}`, borderRadius: 12, padding: 24, textAlign: "center", color: T.text3, fontSize: 12, background: T.panel2 }}>
                      【占位符：角色详情】
                      <div style={{ marginTop: 6, fontSize: 11, color: T.text3 }}>暂无角色，先到角色管理创建</div>
                    </div>
                  );
                }
                const emo = emotionInfo(focusCh.current_status || "");
                const bound = portraits.filter((p) => p.character_id === focusCh.id && p.story_id === currentStory?.id);
                const p0 = bound[0];
                return (
                  <div style={{ border: `1px solid ${emo.label ? emo.color : T.line}`, borderRadius: 12, background: T.panel2, overflow: "hidden", boxShadow: emo.label ? `0 0 12px ${emo.color}44` : "none" }}>
                    <div style={{ display: "flex", gap: 11, padding: 12, alignItems: "center", borderBottom: `1px solid ${T.lineSoft}` }}>
                      <div style={{ width: 46, height: 46, flex: "0 0 46px", borderRadius: 12, overflow: "hidden", background: emo.label ? `linear-gradient(160deg, ${emo.color}88, #22293a)` : "linear-gradient(135deg,#7b5cff,#b45cff)", display: "flex", alignItems: "center", justifyContent: "center", fontSize: 22 }}>
                        {p0?.path ? <img src={convertFileSrc(p0.path)} alt="" style={{ width: "100%", height: "100%", objectFit: "cover", display: "block" }} /> : <span>🧑</span>}
                      </div>
                      <div>
                        <div style={{ fontSize: 13.5, fontWeight: 650, display: "flex", alignItems: "center", gap: 6 }}>
                          {focusCh.name}
                          <span style={{ fontSize: 10, padding: "1px 6px", borderRadius: 5, background: T.accentSoft, color: T.accent, fontWeight: 600 }}>焦点</span>
                        </div>
                        <div style={{ fontSize: 11, color: T.text3, marginTop: 3, display: "flex", alignItems: "flex-start", gap: 6, flexWrap: "wrap", lineHeight: 1.5 }}>
                          {emo.label && <span style={{ fontSize: 10, padding: "1px 8px", borderRadius: 10, color: "#fff", background: emo.color, flex: "0 0 auto" }}>{emo.label}</span>}
                          <span>{focusCh.current_status || "暂无状态"}</span>
                        </div>
                      </div>
                    </div>
                    <div style={{ padding: "10px 12px 8px", fontSize: 12, color: T.text2, lineHeight: 1.5 }}>
                      <div style={{ fontSize: 10.5, color: T.text3, letterSpacing: 0.6, marginBottom: 4 }}>关系</div>
                      {(() => {
                        const rels = relations.filter((r) => r.from_character_id === focusCh.id || r.to_character_id === focusCh.id);
                        if (rels.length === 0) return <div style={{ fontSize: 11, color: T.text3 }}>暂无关系</div>;
                        const nameOf = (id: number) => chars.find((c) => c.id === id)?.name ?? `#${id}`;
                        return (
                          <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                            {rels.map((r) => {
                              const other = r.from_character_id === focusCh.id ? r.to_character_id : r.from_character_id;
                              const arrow = r.direction === "双向" ? "⟷" : r.from_character_id === focusCh.id ? "→" : "←";
                              return (
                                <div key={r.id} style={{ fontSize: 11.5, color: T.text2, display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
                                  <span>{r.direction === "双向" ? `${focusCh.name} ${arrow} ${nameOf(other)}` : r.from_character_id === focusCh.id ? `${focusCh.name} ${arrow} ${nameOf(other)}` : `${nameOf(other)} ${arrow} ${focusCh.name}`}</span>
                                  <span style={{ fontSize: 10, padding: "1px 7px", borderRadius: 9, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2 }}>{r.relation_type}{r.strength ? ` ${r.strength}` : ""} · {r.direction}</span>
                                </div>
                              );
                            })}
                          </div>
                        );
                      })()}
                    </div>
                    <div style={{ padding: "0 12px 12px", fontSize: 12, color: T.text2, lineHeight: 1.5 }}>
                      <div style={{ fontSize: 10.5, color: T.text3, letterSpacing: 0.6, marginBottom: 4 }}>角色设定</div>
                      {focusCh.profile || "（无设定）"}
                    </div>
                    <div style={{ padding: "11px 12px", borderTop: `1px solid ${T.lineSoft}`, display: "flex", flexWrap: "wrap", gap: 6 }}>
                      <span style={{ fontSize: 10.5, padding: "3px 8px", borderRadius: 11, border: `1px dashed ${T.line}`, background: "#191c22", color: T.text3 }}>属性数值条（占位）</span>
                      <span style={{ fontSize: 10.5, padding: "3px 8px", borderRadius: 11, border: `1px dashed ${T.line}`, background: "#191c22", color: T.text3 }}>状态 Buff（占位）</span>
                    </div>
                  </div>
                );
              })()}
            </div>

            {/* 关系图谱（父节点为中心 · 两层 · 单向） */}
            <div>
              <div style={{ fontSize: 10.5, color: T.text3, letterSpacing: 1.1, marginBottom: 8, display: "flex", alignItems: "center", gap: 6 }}>
                <span>🕸 关系图谱</span>
                <span style={{ marginLeft: "auto" }}>{relations.length} 条关系</span>
              </div>
              {chars.length >= 1 && (
                <div style={{ display: "flex", flexWrap: "wrap", gap: 4, marginBottom: 8 }}>
                  {chars.map((c) => {
                    const active = (graphRootId ?? chars.find((x) => x.current_status)?.id ?? chars[0]?.id) === c.id;
                    return (
                      <span key={c.id} onClick={() => setGraphRootId(c.id)} style={{ fontSize: 10.5, padding: "2px 8px", borderRadius: 10, cursor: "pointer", border: `1px solid ${active ? T.accent : T.line}`, background: active ? T.accentSoft : "#191c22", color: active ? T.accent : T.text2, fontWeight: active ? 600 : 400 }}>
                        {c.name}
                      </span>
                    );
                  })}
                </div>
              )}
              {(() => {
                const rootId = graphRootId ?? chars.find((x) => x.current_status)?.id ?? chars[0]?.id ?? null;
                const root = chars.find((c) => c.id === rootId) ?? null;
                // 只取与父节点直接相关的关系（两层：中心 + 直接关联）
                const rels = relations.filter((r) => r.from_character_id === rootId || r.to_character_id === rootId);
                const nodeIds = new Set<number>();
                rels.forEach((r) => {
                  if (r.from_character_id !== rootId) nodeIds.add(r.from_character_id);
                  if (r.to_character_id !== rootId) nodeIds.add(r.to_character_id);
                });
                const nodes = chars.filter((c) => nodeIds.has(c.id));
                if (!root) {
                  return (
                    <div style={{ border: `1px dashed ${T.line}`, borderRadius: 12, padding: "32px 12px", textAlign: "center", color: T.text3, fontSize: 12, background: "#191c22" }}>暂无角色，先在角色管理里添加</div>
                  );
                }
                if (rels.length === 0 || nodes.length === 0) {
                  return (
                    <div style={{ border: `1px dashed ${T.line}`, borderRadius: 12, padding: "32px 12px", textAlign: "center", color: T.text3, fontSize: 12, background: "#191c22" }}>
                      “{root.name}”暂无直接关系，先在关系设定里添加
                    </div>
                  );
                }
                const rootPos = { x: 134, y: 102 };
                const nodePos: Record<number, { x: number; y: number }> = {};
                nodes.forEach((c, i) => {
                  const a = (2 * Math.PI * i) / nodes.length - Math.PI / 2;
                  nodePos[c.id] = { x: 134 + 72 * Math.cos(a), y: 102 + 72 * Math.sin(a) };
                });
                return (
                  <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: "#191c22", padding: 10, overflow: "hidden" }}>
                    <svg width="100%" height={205} viewBox="0 0 268 205" style={{ display: "block" }}>
                      <defs>
                        <marker id="arrEnd" markerWidth="7" markerHeight="7" refX="6" refY="3" orient="auto" markerUnits="userSpaceOnUse">
                          <path d="M0,0 L7,3 L0,6 Z" fill={T.accent} />
                        </marker>
                      </defs>
                      {/* 单向关系线（from → to，双向暂不画反向虚线） */}
                      {rels.map((r) => {
                        const p1 = r.from_character_id === rootId ? rootPos : nodePos[r.from_character_id];
                        const p2 = r.to_character_id === rootId ? rootPos : nodePos[r.to_character_id];
                        if (!p1 || !p2) return null;
                        const dx = p2.x - p1.x, dy = p2.y - p1.y;
                        const len = Math.sqrt(dx * dx + dy * dy) || 1;
                        const r1 = r.from_character_id === rootId ? 20 : 18;
                        const r2 = r.to_character_id === rootId ? 20 : 18;
                        const sx = p1.x + (dx / len) * r1;
                        const sy = p1.y + (dy / len) * r1;
                        const ex = p2.x - (dx / len) * r2;
                        const ey = p2.y - (dy / len) * r2;
                        return (
                          <g key={r.id}>
                            <line x1={sx} y1={sy} x2={ex} y2={ey} stroke={T.accent} strokeWidth={2} markerEnd="url(#arrEnd)" />
                            <text x={(sx + ex) / 2} y={(sy + ey) / 2 - 5} textAnchor="middle" fontSize="10" fill="#fff" fontWeight={600} paintOrder="stroke" stroke="#191c22" strokeWidth={3} style={{ userSelect: "none" }}>
                              {r.relation_type}{r.strength ? ` ${r.strength}` : ""}
                            </text>
                          </g>
                        );
                      })}
                      {/* 中心父节点 */}
                      <circle cx={rootPos.x} cy={rootPos.y} r={20} fill={T.accentSoft} stroke={T.accent} strokeWidth={1.6} />
                      <text x={rootPos.x} y={rootPos.y + 3.5} textAnchor="middle" fontSize="10.5" fill={T.accent} fontWeight={700} style={{ userSelect: "none" }}>{root.name}</text>
                      {/* 一层关联角色 */}
                      {nodes.map((c) => (
                        <g key={c.id}>
                          <circle cx={nodePos[c.id].x} cy={nodePos[c.id].y} r={18} fill={T.panel3} stroke={T.line} strokeWidth={1.2} />
                          <text x={nodePos[c.id].x} y={nodePos[c.id].y + 3.5} textAnchor="middle" fontSize="10" fill={T.text1} fontWeight={600} style={{ userSelect: "none" }}>{c.name}</text>
                        </g>
                      ))}
                    </svg>
                  </div>
                );
              })()}
              <div style={{ marginTop: 10, border: `1px solid ${T.line}`, borderRadius: 8, background: T.panel2, padding: "9px 10px" }}>
                <div style={{ fontSize: 11, color: T.text2, marginBottom: 7 }}>🔀 剧情分支预测（占位）</div>
                <div style={{ fontSize: 11, color: T.text3, borderLeft: `2px solid ${T.line}`, padding: "5px 8px", marginBottom: 5 }}>A · 分支候选（后置）</div>
                <div style={{ fontSize: 11, color: T.text3, borderLeft: `2px solid ${T.line}`, padding: "5px 8px" }}>B · 分支候选（后置）</div>
              </div>
            </div>
          </div>
        </div>

        {/* 设置弹窗 */}
        {showSettings && (
          <div style={{ position: "fixed", inset: 0, background: "rgba(8,9,12,.62)", backdropFilter: "blur(3px)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 100 }}>
            <div style={{ width: 560, maxHeight: "80vh", overflowY: "auto", background: T.panel, border: `1px solid ${T.line}`, borderRadius: 16, boxShadow: "0 24px 70px rgba(0,0,0,.6)" }}>
              <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "16px 20px", borderBottom: `1px solid ${T.line}` }}>
                <div style={{ width: 30, height: 30, borderRadius: 9, background: T.accentSoft, display: "flex", alignItems: "center", justifyContent: "center", color: T.accent, fontSize: 15 }}>⚙️</div>
                <div>
                  <h3 style={{ margin: 0, fontSize: 14.5, fontWeight: 650 }}>设置</h3>
                  <p style={{ margin: "2px 0 0", fontSize: 11.5, color: T.text3 }}>系统设置 · API / 请求日志 / 数据管理</p>
                </div>
                <button onClick={() => setShowSettings(false)} style={{ marginLeft: "auto", color: T.text3, cursor: "pointer", padding: 4, borderRadius: 6, border: "none", background: "transparent", fontSize: 14 }}>✕</button>
              </div>
              <div style={{ padding: "18px 20px", display: "flex", flexDirection: "column", gap: 14 }}>
                <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
                  <button onClick={() => setSettingsTab("api")} style={{ padding: "7px 14px", borderRadius: 8, border: `1px solid ${T.line}`, cursor: "pointer", background: settingsTab === "api" ? T.accentSoft : "#191c22", color: settingsTab === "api" ? T.accent : T.text2, fontSize: 12.5, fontWeight: settingsTab === "api" ? 600 : 400 }}>🔑 API 设置</button>
                  <button onClick={() => { setShowSettings(false); setView("debug"); loadDebug(); }} style={{ padding: "7px 14px", borderRadius: 8, border: `1px solid ${T.line}`, cursor: "pointer", background: "#191c22", color: T.text2, fontSize: 12.5 }}>🧾 请求日志</button>
                  <button onClick={() => { setShowSettings(false); setView("cache"); loadCache(); }} style={{ padding: "7px 14px", borderRadius: 8, border: `1px solid ${T.line}`, cursor: "pointer", background: "#191c22", color: T.text2, fontSize: 12.5 }}>💾 数据管理</button>
                </div>
                {settingsTab === "api" && (
                  <div>
                    <div style={{ display: "grid", gridTemplateColumns: "110px 1fr", gap: 8, alignItems: "center" }}>
                      <label style={{ fontSize: 12, color: T.text2 }}>Base URL</label>
                      <input value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder="https://api.deepseek.com/v1" style={{ padding: "8px 11px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontFamily: "monospace", fontSize: 12, outline: "none" }} />
                      <label style={{ fontSize: 12, color: T.text2 }}>模型名</label>
                      <input value={modelName} onChange={(e) => setModelName(e.target.value)} placeholder="deepseek-chat" style={{ padding: "8px 11px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontFamily: "monospace", fontSize: 12, outline: "none" }} />
                      <label style={{ fontSize: 12, color: T.text2 }}>API Key</label>
                      <input type="password" value={apiKeyInput} onChange={(e) => setApiKeyInput(e.target.value)} placeholder="（留空则不修改已有 Key）" style={{ padding: "8px 11px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontFamily: "monospace", fontSize: 12, outline: "none" }} />
                      <label style={{ fontSize: 12, color: T.text2 }}>上下文窗口</label>
                      <div>
                        <div style={{ display: "flex", gap: 6, flexWrap: "wrap", marginBottom: 6 }}>
                          {[
                            { label: "DeepSeek flash / v4-pro · 1M", v: "1000000" },
                            { label: "GPT-4o 系 · 128k", v: "128000" },
                            { label: "保守档 · 32k", v: "32768" },
                          ].map((p) => (
                            <button key={p.v} onClick={() => setCtxWindowInput(p.v)} style={{ height: 24, padding: "0 8px", borderRadius: 6, border: `1px solid ${ctxWindowInput === p.v ? T.accent : T.line}`, background: ctxWindowInput === p.v ? "rgba(255,122,24,.12)" : "transparent", color: ctxWindowInput === p.v ? T.accent : T.text3, cursor: "pointer", fontSize: 10.5 }}>{p.label}</button>
                          ))}
                        </div>
                        <input value={ctxWindowInput} onChange={(e) => setCtxWindowInput(e.target.value.replace(/[^0-9]/g, ""))} placeholder="模型官方上下文长度，如 1000000" style={{ width: "100%", boxSizing: "border-box", padding: "8px 11px", borderRadius: 8, border: `1px solid ${cwInvalid || cwWarn ? "rgba(240,85,77,.55)" : T.line}`, background: "#191c22", color: T.text1, fontFamily: "monospace", fontSize: 12, outline: "none" }} />
                        <div style={{ fontSize: 10.5, color: cwInvalid || cwWarn ? "#ff8c85" : T.text3, marginTop: 4, lineHeight: 1.6 }}>
                          {cwInvalid ? "⚠ 请填一个大于 0 的整数" : cwWarn ? "⚠ 建议 ≥ 32,768（低于此值历史几乎放不下；填模型官方值即可）" : "填模型官方上下文长度；软件只吃其中一部分（见下方系数）"}
                        </div>
                      </div>
                      <label style={{ fontSize: 12, color: T.text2 }}>预算系数</label>
                      <div>
                        <input value={reserveRatio} onChange={(e) => setReserveRatio(e.target.value.replace(/[^0-9.]/g, ""))} placeholder="0.8" style={{ width: 90, padding: "8px 11px", borderRadius: 8, border: `1px solid ${!(Number(reserveRatio) > 0 && Number(reserveRatio) <= 1) ? "rgba(240,85,77,.55)" : T.line}`, background: "#191c22", color: T.text1, fontFamily: "monospace", fontSize: 12, outline: "none" }} />
                        <span style={{ fontSize: 10.5, color: T.text3, marginLeft: 8 }}>0.8 = 只吃窗口的 80%，留 20% 保底防突发（0~1）</span>
                      </div>
                      <label style={{ fontSize: 12, color: T.text2 }}>每日上限</label>
                      <div>
                        <input value={nRecentInput} onChange={(e) => setNRecentInput(e.target.value.replace(/[^0-9]/g, ""))} placeholder="10" style={{ width: 90, padding: "8px 11px", borderRadius: 8, border: `1px solid ${!(Number(nRecentInput) > 0) ? "rgba(240,85,77,.55)" : T.line}`, background: "#191c22", color: T.text1, fontFamily: "monospace", fontSize: 12, outline: "none" }} />
                        <span style={{ fontSize: 10.5, color: T.text3, marginLeft: 8 }}>条/天；由预算倒推的 N 会被它封顶（跨天时"近三天"各自的上限）</span>
                      </div>
                      <label style={{ fontSize: 12, color: T.text2 }}>单次最大输出</label>
                      <div>
                        <input value={maxTokensInput} onChange={(e) => setMaxTokensInput(e.target.value.replace(/[^0-9]/g, ""))} placeholder="4000" style={{ width: 90, padding: "8px 11px", borderRadius: 8, border: `1px solid ${!(Number(maxTokensInput) > 0) ? "rgba(240,85,77,.55)" : T.line}`, background: "#191c22", color: T.text1, fontFamily: "monospace", fontSize: 12, outline: "none" }} />
                        <span style={{ fontSize: 10.5, color: T.text3, marginLeft: 8 }}>token；会连同安全余量（800）一起从预算里预留出去</span>
                      </div>
                    </div>
                    <div style={{ marginTop: 10, padding: "8px 10px", borderRadius: 8, background: "rgba(255,122,24,.07)", border: "1px solid rgba(255,122,24,.25)", fontSize: 11, color: T.text2, lineHeight: 1.7 }}>
                      按当前设置：可用输入预算 = {fmtNum(Math.max(Math.round(cwNumber * (Number(reserveRatio) || 0.8)) - (Number(maxTokensInput) || 4000) - 800, 0))} token
                      <span style={{ color: T.text3 }}>（= 窗口 {fmtNum(cwNumber)} × {Number(reserveRatio) || 0.8} − 输出 {fmtNum(Number(maxTokensInput) || 4000)} − 安全余量 800）</span>
                    </div>
                    <div style={{ marginTop: 12, display: "flex", gap: 8 }}>
                      <button onClick={saveSettings} style={{ height: 32, padding: "0 16px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, cursor: "pointer", fontSize: 12.5 }}>保存设置</button>
                      <button onClick={runTest} disabled={testBusy} style={{ height: 32, padding: "0 16px", borderRadius: 8, border: "none", background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505", cursor: "pointer", fontSize: 12.5, fontWeight: 650 }}>{testBusy ? "测试中..." : "测试连接"}</button>
                    </div>
                    {testResult && (
                      <div style={{ marginTop: 10, padding: 8, background: testResult.ok ? "rgba(55,201,120,.1)" : "rgba(240,85,77,.1)", border: `1px solid ${testResult.ok ? "rgba(55,201,120,.4)" : "rgba(240,85,77,.4)"}`, borderRadius: 6, fontSize: 13, color: testResult.ok ? T.ok : "#ff8c85" }}>
                        <div>{testResult.ok ? "✅" : "❌"} {testResult.detail}</div>
                        {testResult.reply && <div style={{ marginTop: 4, color: T.text1 }}>AI 回复：<b>{testResult.reply}</b></div>}
                      </div>
                    )}
                  </div>
                )}
              </div>
            </div>
          </div>
        )}

        {/* 导出粒度弹窗 */}
        {showExport && (
          <div style={{ position: "fixed", inset: 0, background: "rgba(8,9,12,.62)", backdropFilter: "blur(3px)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 100 }}>
            <div style={{ width: 460, maxWidth: "90vw", background: T.panel, border: `1px solid ${T.line}`, borderRadius: 14, padding: 20, boxSizing: "border-box" }}>
              <div style={{ display: "flex", alignItems: "center", marginBottom: 16 }}>
                <h3 style={{ margin: 0, fontSize: 15 }}>📥 导出剧情（TXT）</h3>
                <button onClick={() => setShowExport(false)} style={{ marginLeft: "auto", fontSize: 14, padding: "4px 10px", cursor: "pointer", color: T.text2 }}>✕</button>
              </div>
              <div style={{ marginBottom: 14 }}>
                <div style={{ fontSize: 12, color: T.text3, marginBottom: 6 }}>节点范围</div>
                <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
                  {([["all", "全部节点"], ["current", "当前节点"], ["range", "起止区间"]] as const).map(([v, l]) => (
                    <button key={v} onClick={() => setExpScope(v)} style={{ padding: "6px 14px", borderRadius: 7, cursor: "pointer", fontSize: 12.5, border: `1px solid ${T.line}`, background: expScope === v ? T.accentSoft : "#191c22", color: expScope === v ? T.accent : T.text2, fontFamily: "inherit" }}>{l}</button>
                  ))}
                </div>
                {expScope === "range" && (
                  <div style={{ display: "flex", gap: 8, alignItems: "center", marginTop: 8, flexWrap: "wrap" }}>
                    <select value={expFrom} onChange={(e) => setExpFrom(Number(e.target.value))} style={{ padding: 6, fontSize: 12.5 }}>{dayNodes.map((n) => <option key={n.id} value={dayNoOf(n)}>{n.date_label}</option>)}</select>
                    <span style={{ fontSize: 12, color: T.text3 }}>~</span>
                    <select value={expTo} onChange={(e) => setExpTo(Number(e.target.value))} style={{ padding: 6, fontSize: 12.5 }}>{dayNodes.map((n) => <option key={n.id} value={dayNoOf(n)}>{n.date_label}</option>)}</select>
                    <span style={{ fontSize: 11, color: T.text3 }}>（不含插曲 / 回忆）</span>
                  </div>
                )}
              </div>
              <div style={{ marginBottom: 16 }}>
                <div style={{ fontSize: 12, color: T.text3, marginBottom: 6 }}>内容模式（用户输入始终保留）</div>
                <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
                  {([["all", "全部"], ["narr", "仅旁白（AI）"], ["chat", "仅对话（AI）"]] as const).map(([v, l]) => (
                    <button key={v} onClick={() => setExpMode(v)} style={{ padding: "6px 14px", borderRadius: 7, cursor: "pointer", fontSize: 12.5, border: `1px solid ${T.line}`, background: expMode === v ? T.accentSoft : "#191c22", color: expMode === v ? T.accent : T.text2, fontFamily: "inherit" }}>{l}</button>
                  ))}
                </div>
              </div>
              <div style={{ marginBottom: 16 }}>
                <div style={{ fontSize: 12, color: T.text3, marginBottom: 6 }}>导出格式</div>
                <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
                  {([["txt", "TXT 存档"], ["md", "MD 可编辑"], ["html", "HTML 阅读/打印"]] as const).map(([v, l]) => (
                    <button key={v} onClick={() => setExpFmt(v)} style={{ padding: "6px 14px", borderRadius: 7, cursor: "pointer", fontSize: 12.5, border: `1px solid ${T.line}`, background: expFmt === v ? T.accentSoft : "#191c22", color: expFmt === v ? T.accent : T.text2, fontFamily: "inherit" }}>{l}</button>
                  ))}
                </div>
              </div>
              <div style={{ display: "flex", alignItems: "center", gap: 10, justifyContent: "flex-end", borderTop: `1px solid ${T.lineSoft}`, paddingTop: 14 }}>
                <span style={{ fontSize: 11, color: T.text3 }}>输出位置：exports\story · 文件名带时间戳</span>
                <button onClick={doExport} style={{ padding: "8px 22px", border: "none", borderRadius: 8, background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505", fontSize: 13, fontWeight: 650, cursor: "pointer", fontFamily: "inherit" }}>导出</button>
              </div>
            </div>
          </div>
        )}
      </div>
    );
}
export default App;
