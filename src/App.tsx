import { useEffect, useRef, useState, type ReactElement } from "react";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { pluginCaps, pluginConfigKey } from "./plugins";
import type { GenDescriptor, GenProbe, CloudForm } from "./plugins";
import { LICENSE_NOTICE, NOTICE_VERSION, REQUIRED_NOTICES } from "./compliance";
import type {
  BranchDto,
  Character,

  CharacterUsage,
  PortraitItem,
  Relation,
  SnapDiff,
  SnapItemChange,
  Story,
  TimelineNodeDto,
  WorldSetting,
  DebugReq,
  EvalT,
  Proposal,
  Snapshot,
  AuditLog,
  EventLogItem,
} from "./types";
import { emotionInfo } from "./lib/chars";
import {

  PLUGIN_REGISTRY,

} from "./constants";
import { T } from "./theme";
import {
  fmtNum,

} from "./lib/parse";
import { renderBold } from "./lib/rich";
import { TempPage } from "./views/TempPage";
import { CompliancePage } from "./views/CompliancePage";
import { TimelinePage } from "./views/TimelinePage";
import { buildTreeRows } from "./lib/timelineTree";
import { PortraitPage } from "./views/PortraitPage";
import { CharsPage } from "./views/CharsPage";
import { WorldPage } from "./views/WorldPage";
import { ParamsPage } from "./views/ParamsPage";
import { ProposalsPage } from "./views/ProposalsPage";
import { PluginsPage } from "./views/PluginsPage";
import { StoryPanel } from "./views/StoryPanel";
import { StoryInput } from "./views/StoryInput";
import { StoriesPage } from "./views/StoriesPage";
import { Sidebar } from "./views/Sidebar";

function App() {
  // ===== 故事库（M1）=====
  const [stories, setStories] = useState<Story[]>([]);
  const [currentStory, setCurrentStory] = useState<Story | null>(null);
  const [showNew, setShowNew] = useState(false);
  const [newTitle, setNewTitle] = useState("");
  const [newOrigin, setNewOrigin] = useState("2026-01-01");
  const [msg, setMsg] = useState("");
  /// 中间工作区滚动容器：**切页时回到顶部**。
  /// 不重置的话，从长页面（如「待确认设定」）切过去时，
  /// 页面顶部的提示信息（全局 msg）会停在屏幕外，表现成"点了没反应"。
  const mainScrollRef = useRef<HTMLDivElement | null>(null);

  // ===== 设置（M0/M2）=====
  const [showSettings, setShowSettings] = useState(false);
  const [baseUrl, setBaseUrl] = useState("");
  const [modelName, setModelName] = useState("");
  const [apiKeyInput, setApiKeyInput] = useState("");
  // 主模型 Key 是否已在系统凭据库中（界面只显示"已保存"，不显示明文）
  const [hasApiKey, setHasApiKey] = useState(false);
  const [testBusy, setTestBusy] = useState(false);
  const [testResult, setTestResult] = useState<{ ok: boolean; detail: string; reply?: string } | null>(null);

  // ===== 请求记录（调试用；只读，不可回滚）=====
  const [debugList, setDebugList] = useState<DebugReq[]>([]);
  const [selectedDebug, setSelectedDebug] = useState<DebugReq | null>(null);

  // ===== 角色库（工作区内）=====
  const [chars, setChars] = useState<Character[]>([]);
  const [charUsage, setCharUsage] = useState<CharacterUsage[]>([]);
  // ===== 角色管理页（A 卡墙 / D 关系图谱 / G 新建表单）=====
  const [charsLoading, setCharsLoading] = useState(false);
  const [charGraphRootId, setCharGraphRootId] = useState<number | null>(null);

  // ===== 世界观（工作区内）=====
  const [view, setView] = useState<"chars" | "world" | "relations" | "story" | "proposals" | "temp" | "timeline" | "stories" | "plugins" | "portrait" | "compliance" | "params" | "home">("home");
  const [navMode, setNavMode] = useState<"旁白" | "对话">("对话"); // 旁白/对话两态
  const [myRole, setMyRole] = useState("自由"); // 对话模式：我扮演的角色
  const [aiRole, setAiRole] = useState("自由"); // 对话模式：AI 扮演的角色
  // 敏感词过滤：**内置常开，不提供开关**（拦截输入 + 过滤 AI 输出）。
  // 保留常量名以便下游判断逻辑不变；不再有 setState —— 界面上的按钮改为状态指示。
  const sensitiveFilter = true;
  const [constraints, setConstraints] = useState<string[]>(() => { try { return JSON.parse(localStorage.getItem("ai.constraints") || "[]"); } catch { return []; } });   // 剧情约束（注入提示词，持久化）
  const [lenLevel, setLenLevel] = useState<"简单" | "详细">("简单");   // 篇幅：简单描写 / 详细展开
  const [memoryCtx, setMemoryCtx] = useState(true);                     // 记忆上下文：是否携带历史
  // ===== RAG ①：回顾 =====
  /// 界面「🔍 回顾」指定的某一天（node_id）。有值时后端**强制**把那天拼进提示词。
  const [recallDay, setRecallDay] = useState<number | null>(null);
  /// 「🔍 回顾」弹窗是否打开
  const [showRecallPicker, setShowRecallPicker] = useState(false);
  /// 可选的天（本分支可见 + 有日摘要）
  const [recallDays, setRecallDays] = useState<{ node_id: number; date_label: string; day: number | null; msg_count: number; preview: string; has_summary: boolean }[]>([]);
  // 导演指令（沉浸对话专用，一次性：附加到下一次发送后自动清空）
  const [directors, setDirectors] = useState<string[]>(() => { try { return JSON.parse(localStorage.getItem("ai.directors") || "[]"); } catch { return []; } });
  const [timelineView, setTimelineView] = useState<"list" | "cal" | "tree">("list");   // 时间线视图：列表/日历/树状图
  const [calOffset, setCalOffset] = useState(0);                               // 日历翻页偏移（月）
  // C 子项（时间流逝）：点区间条 → 在指定天数插入事件
  const [gapPrompt, setGapPrompt] = useState<{ from: number; to: number; day: string; title: string } | null>(null);
  const [showExport, setShowExport] = useState(false);                        // 导出粒度弹窗
  const [expScope, setExpScope] = useState<"all" | "current" | "range">("all");
  const [expFrom, setExpFrom] = useState(1);
  const [expTo, setExpTo] = useState(1);
  const [expMode, setExpMode] = useState<"all" | "narr" | "chat">("all");
  const [expFmt, setExpFmt] = useState<"txt" | "md" | "html">("txt");
  const [pDetail, setPDetail] = useState<number | null>(null);
  const [portraits, setPortraits] = useState<PortraitItem[]>([]);
  const loadPortraits = async () => {
    try { setPortraits(await invoke<PortraitItem[]>("portrait_list")); } catch (e) { console.error(e); setMsg("❌ 加载素材失败: " + String(e)); }
  };
  useEffect(() => { loadPortraits(); }, []);
  // ===== 生成参数页（左侧导航：立绘素材下方）=====
  const [ctxWindow, setCtxWindow] = useState(32768);
  // 上下文预算（A 子项）：设置页三个输入 + 本地即时校验
  const [ctxWindowInput, setCtxWindowInput] = useState("32768");
  const [reserveRatio, setReserveRatio] = useState("0.8");
  const [maxTokensInput, setMaxTokensInput] = useState("4000");
  const [nRecentInput, setNRecentInput] = useState("10");   // 每日上限：N 的封顶值
  const [autoAnalyze, setAutoAnalyze] = useState(true);      // 第三批：回复后自动分析演进
  const cwNumber = Number(ctxWindowInput) || 0;
  const cwWarn = cwNumber > 0 && cwNumber < 32768;
  const cwInvalid = ctxWindowInput.trim() !== "" && (!cwNumber || cwNumber <= 0);
  const [pParamId, setPParamId] = useState<number | null>(null);
  const [pDebugId, setPDebugId] = useState<number | null>(null);
  // 「未开放的生成参数」折叠区（原来是 5 个散落的虚框，很杂）
  const [pParamForm, setPParamForm] = useState({ prompt: "", negative: "", seed: "", steps: "", cfg: "", model: "" });
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

  // ===== 剧情（工作区内）=====
  const [node, setNode] = useState<{ id: number; date_label: string } | null>(null);

  // 第五批 ③：分支状态。`currentBranchId = null` 表示主线。
  //
  // ⚠ **必须声明在"用到它的派生值"之前**：下面 `visibleForBranch` / `branchScopedNodes`
  // 会在**每次渲染时立刻求值**，若 state 声明在后面就撞上 TDZ
  // （实测白屏：`ReferenceError: Cannot access 'currentBranchId' before initialization`）。
  // 这类错误只在运行时报，`tsc` 检查不出来。
  const [branches, setBranches] = useState<BranchDto[]>([]);
  const [currentBranchId, setCurrentBranchId] = useState<string | null>(null);
  const [branchDialog, setBranchDialog] = useState<{ open: boolean; name: string; forkNodeId: number | null; busy: boolean }>(
    { open: false, name: "", forkNodeId: null, busy: false }
  );
  // 第五批「主生成链路」修复（《现状说明》5.2）：记录本次生成的 requestId，
  // "停止"按钮带着它调 cancel_generation。null = 当前没有在生成。
  const [genRequestId, setGenRequestId] = useState<string | null>(null);
  const genRequestRef = useRef<string | null>(null);   // 给事件回调读（避免闭包拿到旧 state）
  // 第五批（《现状说明》5.3）：**卸载时要跑的清理**（注销流式监听器）
  const activeCleanupRef = useRef<(() => void) | null>(null);

  // 第五批 ④：分支改名 / 备注（管理平行故事）。null = 弹窗关闭。
  const [branchEdit, setBranchEdit] = useState<{ id: string; name: string; note: string; busy: boolean } | null>(null);
  const [timelineNodes, setTimelineNodes] = useState<TimelineNodeDto[]>([]);
  const [messages, setMessages] = useState<{ id: number; role: string; content: string; mode: string; created_at: string }[]>([]);
  const [input, setInput] = useState("");
  const [editingId, setEditingId] = useState<number | null>(null);
  const [editText, setEditText] = useState("");

  // ===== 关系（工作区内）=====
  const [relations, setRelations] = useState<Relation[]>([]);
  // 关系图谱父节点（null = 自动取焦点角色/第一个角色）
  const [graphRootId, setGraphRootId] = useState<number | null>(null);
  // 当前角色卡焦点角色（null = 默认第一个有状态的角色；点击右侧立绘区才切换）
  const [focusCharId, setFocusCharId] = useState<number | null>(null);

  // ===== 设定演进提案（M6）=====
  const [proposals, setProposals] = useState<Proposal[]>([]);
  const [evals, setEvals] = useState<EvalT[]>([]);
  const [evalBusy, setEvalBusy] = useState(false);
  // 插件运行期状态（注册表驱动：加插件只需在 PLUGIN_REGISTRY 里加一条）
  const [pluginState, setPluginState] = useState<Record<string, { enabled: boolean; panelOpen: boolean }>>(
    Object.fromEntries(PLUGIN_REGISTRY.map((d) => [d.id, { enabled: false, panelOpen: false }]))
  );
  const setPluginFlag = (id: string, patch: Partial<{ enabled: boolean; panelOpen: boolean }>) =>
    setPluginState((prev) => ({ ...prev, [id]: { ...(prev[id] ?? { enabled: false, panelOpen: false }), ...patch } }));
  // 第四批 ⑥ P1：立绘生成通道（后端注册表下发，如实报告每个通道是否已实装）
  const [genList, setGenList] = useState<GenDescriptor[]>([]);
  const [genSel, setGenSel] = useState("local_sdwebui");
  const [genUrl, setGenUrl] = useState("http://127.0.0.1:7860");
  const [genProbing, setGenProbing] = useState(false);
  const [genProbe, setGenProbe] = useState<GenProbe | null>(null);
  const [genBusy, setGenBusy] = useState(false);
  const [genSummary, setGenSummary] = useState("");
  // 生成结果一律烧录「AI」标识（显式角标 + 隐式元数据）：**固定行为，界面不提供开关**（用户 2026-10-12 定）。
  // 理由：AI 生成内容带标识是法规要求（《人工智能生成合成内容标识办法》2025-09-01 施行）；
  // 与其给一个"关掉标识"的按钮，不如让它成为软件的既定行为。实施位置见下面的 runGenerate。
  // 合规中心：「我已了解」确认（持久化）+ 自查清单勾选（本次会话内）
  const [complianceAck, setComplianceAck] = useState(false);
  const [complianceNotice, setComplianceNotice] = useState(false);   // 首次启动的合规提示（一次性）
  // 第五批 ①：时间线树的折叠状态。
  // 现在**用不到**（线性故事一律全部展开，不显示折叠控件），但第 ③ 步分支系统会用到：
  // 分叉出现后，用户需要"只展开我正在看的那条线、收起别的分支"。
  // 保留它是为了到时候不必再改 treeRows 的判定逻辑 —— 那句 `!hasFork || treeExpanded.has(id)` 已经写好了。
  /**
   * 树控件里**被用户手动收起**的节点集合。
   *
   * 语义是"**默认全部展开**，只记例外" —— 反过来做（记"已展开的"）会踩一个坑：
   * 集合初始为空 → 有分叉时 `treeExpanded.has(id)` 全为 false → **整棵树只剩根节点**
   * （用户实测：有了 5 条分支后，时间维里只剩「第 1 天」）。
   * 用 ref 而不是 state：折叠是纯展示状态，改它不需要触发数据重载，但需要跨渲染保持。
   */
  const treeCollapsedRef = useRef<Set<number>>(new Set());
  const [, forceTreeRedraw] = useState(0);

  /**
   * 第五批 ①：把时间线节点整理成"树的行"（缩进列表，形态 A）。
   *
   * 与后端 `timeline::flatten_tree` 保持**同一套规则**：
   * - 同层按 (sort_order, id) 排序
   * - 起点 = 所有 parent_id 为空/指向不存在节点的节点（通常是「第 1 天」）
   * - **没有分叉时视为"全部展开"**；有分叉时只在 expanded 里出现过的节点才展开其子节点
   * - 防环（数据异常时不死循环）
   *
   * 为什么在前端也算一遍：后端只为"装配历史"服务，界面需要的是"带深度的一串 id"，
   * 在前端算省一次往返，而且展开/收起是纯界面状态，不该每次点都问后端。
   */
  /**
   * 树状图的扁平化行。
   *
   * **两个消费方需求不同，必须分开取源**（2026-10-17 用户实测发现的问题）：
   *   · `treeRows`   = 折叠感知 → **树状图表格**用（收起某节点，它的子孙不出现）
   *   · `treeRowsFlat` = 折叠无关 → **时间维区间条**用（否则收起后日期按钮会消失）
   *
   * 原来只有一份 `treeRows`（折叠感知）和一份 `treeRowsAll`（无关），
   * 而树状图表格误用了后者 → "点收起，树不动，倒是区间条动了"。
   */
  const treeRows = buildTreeRows(timelineNodes, treeCollapsedRef.current);
  const treeRowsFlat = buildTreeRows(timelineNodes, null);
  /**
   * **"当前分支能看到的节点"** = 共享历史（`branch_id IS NULL`）+ 当前分支自己的节点。
   *
   * 用户 2026-10-16 定：时间维/列表**只显示当前分支这一条线能看到的节点**，
   * 别的分支的节点**不出现**（原先"显示全树 + 打锁"的做法让用户以为数据错乱：
   * "切到主线还看得到别的分支的第 13~15 天，而且还改不了"）。
   * 想看全部分支的结构 → 切到 **🌲 时间线树状图**（那里永远显示全部）。
   */
  const visibleForBranch = (x: TimelineNodeDto) => (x.branch_id ?? null) === (currentBranchId ?? null);
  /** 分支颜色（主线用中性灰）。树状图与时间维共用。 */
  const branchColor = (bid: string | null | undefined) =>
    bid == null ? "#6B7280" : (branches.find((b) => b.id === bid)?.color ?? "#8B5CF6");
  /** 分支名（主线 / 分支名 / 未知分支回退成 id 前 8 位）。 */
  const branchLabelOf = (bid: string | null | undefined) =>
    bid == null ? "主线" : (branches.find((b) => b.id === bid)?.name ?? (bid.slice(0, 8) + "…"));
  /**
   * **当前这条线"能看到"的节点 id 集合** = 共享历史（沿父链上溯到分叉点及之前）∪ 本分支自己的节点。
   *
   * 为什么要单独算：时间维只画"这条线"的内容，而**共享历史**与**别的分支**必须区别对待 ——
   * 前者要**逐个画出来**（第 1~11 天是共同经历），后者才收成一行"支线"。
   * 曾经把两者混为一谈 → 共享历史被整段跳成"时间流逝 · 第 2~11 天"（用户实测反馈）。
   *
   * 算法：从"本分支最后一个节点"（没有则分叉点）出发，沿 parent_id 上溯取全路径。
   */
  const branchPathIds = (() => {
    const byId = new Map<number, TimelineNodeDto>();
    timelineNodes.forEach((n) => byId.set(n.id, n));
    const sortKey = (a: TimelineNodeDto, b: TimelineNodeDto) => (a.sort_order - b.sort_order) || (a.id - b.id);

    const out = new Set<number>();
    let startId: number | null = null;

    if (currentBranchId != null) {
      // 分支：从**分叉点**出发沿父链上溯 —— 得到"共享历史"
      // （本分支自己的节点是分叉点的**后代**，逐个加进来即可）
      const b = branches.find((x) => x.id === currentBranchId);
      startId = b ? b.fork_node_id : null;
      timelineNodes
        .filter((n) => (n.branch_id ?? null) === currentBranchId)
        .forEach((n) => out.add(n.id));
    } else {
      // 主线：从**最后一个主线节点**出发沿父链上溯
      const m = timelineNodes.filter((n) => n.branch_id == null).sort(sortKey);
      startId = m.length > 0 ? m[m.length - 1].id : null;
    }

    // **沿 parent_id 逐级上溯**：每个节点只有一个父，所以路径是唯一的 ——
    // 这条路**只含真正的祖先**，不会把"同父的兄弟分支"（如主线的第 12 天）卷进来。
    // （早先按 `branch_id` 批量取，结果兄弟节点也被当成共享历史 → 共享历史被整段跳过。）
    let cur = startId;
    let guard = 0;
    while (cur != null && guard++ < 10000) {
      if (out.has(cur)) break;      // 防环
      out.add(cur);
      const node = byId.get(cur);
      cur = node?.parent_id ?? null;
    }
    return out;
  })();
  /** 当前分支能看到的节点（用于列表与时间维） */
  const branchScopedNodes = timelineNodes.filter(visibleForBranch);

  /** 可见节点 id 集合：折叠时子节点不显示（也要从时间线条里过滤掉） */

  const [noticeAgreed, setNoticeAgreed] = useState(false);   // 首次启动弹窗必须勾选才能继续
  const [checklistDone, setChecklistDone] = useState<Set<string>>(new Set());
  const loadComplianceAck = async () => {
    try {
      // 知情同意的判定标准：**确认过的条款版本 == 当前条款版本**。
      // 只存时间戳是不够的：条款改版后旧时间戳依然算数，用户"同意"的其实是旧条款。
      const ver = await invoke<string | null>("get_config", { key: "compliance.acknowledged_version" });
      const ok = (ver ?? "").trim() === NOTICE_VERSION;
      setComplianceAck(ok);
      // 未确认当前版本（含"从没确认过"与"确认的是旧版本"两种情况）→ 弹窗
      if (!ok) setComplianceNotice(true);
    } catch { /* 读不到就不打扰 */ }
  };
  /** 首次启动合规提示：一次性，确认或关闭后写入 compliance.notice_seen */
  const dismissComplianceNotice = async () => {
    setComplianceNotice(false);
    try {
      await invoke("set_config", { key: "compliance.acknowledged_version", value: NOTICE_VERSION });
      await invoke("set_config", { key: "compliance.notice_seen", value: new Date().toLocaleString() });
      setComplianceAck(true);
      setMsg("✅ 已记录：你已阅读并同意当前条款（版本 " + NOTICE_VERSION + "）");
    } catch { /* 忽略 */ }
  };
  const ackCompliance = async () => {
    try {
      await invoke("set_config", { key: "compliance.acknowledged_at", value: new Date().toLocaleString() });
      await invoke("set_config", { key: "compliance.acknowledged_version", value: NOTICE_VERSION });
      setComplianceAck(true);
      setMsg("✅ 已记录：你已阅读合规与责任说明");
    } catch (e) { setMsg("❌ 保存失败: " + String(e)); }
  };

  /** 面板预填（从角色卡 / 素材重出带过来）；stamp 变化才灌入，避免覆盖用户正在编辑的内容 */
  const [genPrefill, setGenPrefill] = useState<{
    stamp: number; prompt?: string; negative?: string; width?: number; height?: number;
    seed?: string; steps?: string; cfg?: string; characterId?: number | null;
    basePortraitId?: number | null; note?: string;
  } | null>(null);

  /** 打开立绘生成面板并（可选）预填 —— 所有「去生成」的入口都走这里 */
  const openGenPanel = (prefill?: { prompt?: string; negative?: string; width?: number; height?: number; seed?: string; steps?: string; cfg?: string; characterId?: number | null; basePortraitId?: number | null; note?: string }) => {
    if (prefill) setGenPrefill({ ...prefill, stamp: Date.now() });
    setView("plugins");
    setGenProbe(null);
    loadGenerators();
    loadCloudConfig();
    setPluginFlag("portrait-gen", { panelOpen: true });
  };

  /** 从角色卡发起：用角色资料拼基础提示词，并自动绑定该角色 */

  /** 从素材发起「重出这张」：带原提示词 / 负向 / 种子 / 尺寸，并挂到重出链上 */
  const [cloudForm, setCloudForm] = useState<CloudForm>({
    base_url: "", model: "", url: "", body_template: "", response_path: "",
    auth_style: "bearer", extra_headers: "", api_key: "", has_api_key: false,
  });
  const [cloudSaving, setCloudSaving] = useState(false);

  // 读已保存的云端配置（Key 只回传"是否已设置"）
  const loadCloudConfig = async () => {
    try {
      const c = await invoke<Record<string, unknown>>("generator_get_cloud_config");
      setCloudForm((prev) => ({
        ...prev,
        base_url: String(c.base_url ?? ""),
        model: String(c.model ?? ""),
        url: String(c.url ?? ""),
        body_template: String(c.body_template ?? ""),
        response_path: String(c.response_path ?? ""),
        auth_style: String(c.auth_style || "bearer"),
        extra_headers: String(c.extra_headers ?? ""),
        api_key: "",                       // 不回显明文
        has_api_key: Boolean(c.has_api_key),
      }));
    } catch (e) { console.error("读云端配置失败：", e); }
  };


  /** 把界面当前值组装成"覆盖值"（留空字段不传，后端会用已保存值） */
  const cloudOverrides = () => ({
    base_url: cloudForm.base_url || null,
    model: cloudForm.model || null,
    url: cloudForm.url || null,
    body_template: cloudForm.body_template || null,
    response_path: cloudForm.response_path || null,
    auth_style: cloudForm.auth_style || null,
    extra_headers: cloudForm.extra_headers || null,
    api_key: cloudForm.api_key || null,
  });

  const loadGenerators = async () => {
    try {
      const list = await invoke<GenDescriptor[]>("generator_list");
      setGenList(list);
      if (list.length > 0 && !list.some((g) => g.id === genSel)) setGenSel(list[0].id);
    } catch (e) { console.error("载入生成通道失败：", e); }
  };

  // 连接测试（只读探测，不产图）

  // 生成 → 逐张入库（入库走既有 portrait_import，严守 R1）
  const runGenerate = async (o: { prompt: string; negative: string; width: number; height: number; batch: number; seed: string; steps: string; cfg: string; characterId: number | null; name: string }) => {
    if (!currentStory) { setMsg("⚠️ 请先进入一个故事（生成立绘需要知道绑定给哪个角色）"); return; }
    setGenBusy(true);
    setGenSummary("");
    try {
      const res = await invoke<{ files: string[]; generator_id: string; seeds: number[]; meta: string }>("generator_generate", {
        generatorId: genSel,
        req: {
          prompt: o.prompt, negative: o.negative, width: o.width, height: o.height,
          steps: o.steps ? Number(o.steps) : null, cfg: o.cfg ? Number(o.cfg) : null,
          seed: o.seed ? Number(o.seed) : null, batch: o.batch, base_portrait_id: null,
        },
        config: cloudOverrides(),
      });
      if (!res.files || res.files.length === 0) { setMsg("⚠️ 通道没有返回图片"); return; }
      // 逐张入库（若开了烧录开关，先把 AI 标识烧进图里 —— 只加标识、不改内容）
      let ok = 0;
      let stamped = 0;
      const names: string[] = [];
      for (let i = 0; i < res.files.length; i++) {
        let filePath = res.files[i];
        // 固定烧录「AI」标识（显式角标 + 隐式元数据）。失败不阻断：按原图入库，内容不受影响。
        try {
          filePath = await invoke<string>("stamp_ai_label", { path: filePath });
          stamped++;
        } catch (e) {
          console.error("烧录 AI 标识失败，按原图入库：", e);
        }
        try {
          const item = await invoke<{ id: number; name: string; ver: string }>("generator_ingest", {
            req: {
              file: filePath,
              name: o.name + (res.files.length > 1 ? ` #${i + 1}` : ""),
              role: "",
              tags: "AI生成",
              group: "AI 生成",
              story_id: currentStory.id,
              character_id: o.characterId,
              generator_id: res.generator_id,
              channel: genList.find((g) => g.id === genSel)?.channel ?? "local",
              prompt: o.prompt, negative: o.negative,
              seed: String(res.seeds[i] ?? res.seeds[0] ?? ""),
              steps: o.steps, cfg: o.cfg, model: "",
              base_portrait_id: null,   // 入库请求字段（写入素材库 derived_from 列）
            },
          });
          ok++;
          names.push(`${item.name}（${item.ver}）`);
        } catch (e) {
          console.error("入库失败：", e);
        }
      }
      setGenSummary(`✅ 已入库 ${ok}/${res.files.length} 张${stamped > 0 ? `（其中 ${stamped} 张已烧录 AI 标识）` : ""}：${names.join("、")}${o.characterId ? "（已绑定该角色）" : ""}  ·  ${res.meta}`);
      await loadPortraits();
      await invoke("generator_cleanup_temp", { keep: [] }).catch(() => {});
    } catch (e) {
      setMsg("❌ 生成失败: " + String(e));
    } finally {
      setGenBusy(false);
    }
  };

  // 插件总开关的配置键（插件化之前就叫 evaluate.auto 的，用 legacy 兼容，老用户设置不丢）
  const PLUGIN_KEY: Record<string, string> = { evaluate: "evaluate.auto" };
  const pluginKey = (id: string) => PLUGIN_KEY[id] ?? pluginConfigKey(id);
  const pluginOn = (id: string) => pluginState[id]?.enabled ?? false;

  // 第三批 ④：AI 回复质量评估

  // 载入评估记录（不需 API）—— 定义在 loadProposals 之前，供插件页入口与自动评估复用

  // ===== ⑤-B：待确认设定（全部入队 + 人工拍板）=====
  /// 开关：设定变更是否必须先拍板（默认 true）
  const [requireApproval, setRequireApproval] = useState(true);
  /// 批量勾选：选中的提案组 id
  const [selGroups, setSelGroups] = useState<Set<number>>(new Set());
  /// **就地**显示"上次分析的结果"。
  ///
  /// 为什么必须有它：全局 `msg` 渲染在滚动区**最顶部**，而本页很长、
  /// 切页时滚动位置不重置 —— 用户点「分析」后提示在屏幕外，
  /// 表现就是"点了没反应"（用户实测反馈）。
  const [analyzeInfo, setAnalyzeInfo] = useState<{ text: string; bad: boolean } | null>(null);
  /// 是否展开"已处理"历史

  const loadRequireApproval = async () => {
    try { setRequireApproval(await invoke<boolean>("get_evolution_require_approval")); } catch { /* 读不到按默认 true */ }
  };

  const loadProposals = async () => {
    try {
      const list = await invoke<Proposal[]>("list_proposals", { storyId: String(currentStory?.id) });
      setProposals(list);
      await loadEventLogs();
      await loadEvals();
    } catch (e) { console.error(e); }
  };

  // 载入评估记录（不需要 API）
  const loadEvals = async () => {
    if (!currentStory) { setEvals([]); return; }
    try {
      const list = await invoke<EvalT[]>("list_reply_evaluations", { storyId: String(currentStory.id), limit: 20 });
      setEvals(list);
    } catch (e) { console.error("载入评估失败：", e); }
  };

  // 手动评估当前节点最后一条 AI 回复（会调 API，有成本）


  /// 注入一条测试提案（不依赖 AI 剧情信号）

  /// ⑤-B：批量落地选中的组（后端整批只打一个快照）



  // ===== M7：设定版本 / 审计 / 事件日志 / 缓存 =====
  const [snapshots, setSnapshots] = useState<Snapshot[]>([]);
  // 第五批 ②：回滚前的差异预览（null = 未打开）。回滚是破坏性操作，必须先看清会失去什么。
  const [snapPreview, setSnapPreview] = useState<{
    version: number; diff: SnapDiff; loading: boolean; error: string;
  } | null>(null);
  // 第五批 ②（B）：本次会话是否已经为"直接编辑"存过临时备份（同一会话只存第一次）
  const backupTakenRef = useRef(false);

  const [auditLogs, setAuditLogs] = useState<AuditLog[]>([]);
  const [eventLogs, setEventLogs] = useState<EventLogItem[]>([]);
  // 「临时文件」页：temp/ 里的 AI 出图中间文件（入库失败/取消会残留）
  type TempFile = { name: string; size: number; age_secs: number };
  const [tempFiles, setTempFiles] = useState<TempFile[]>([]);

  const loadSnapshots = async () => {
    try { setSnapshots(await invoke<Snapshot[]>("list_snapshots", { storyId: String(currentStory?.id) })); } catch (e) { console.error(e); }
  };
  const loadAudit = async () => {
    try { setAuditLogs(await invoke<AuditLog[]>("list_audit_logs", { storyId: String(currentStory?.id) })); } catch (e) { console.error(e); }
  };
  const loadEventLogs = async () => {
    try { setEventLogs(await invoke<EventLogItem[]>("list_event_logs", { storyId: String(currentStory?.id) })); } catch (e) { console.error(e); }
  };
  const loadTemp = async () => {
    try { setTempFiles(await invoke<TempFile[]>("generator_list_temp")); } catch (e) { console.error(e); }
  };
  /// 清理 temp/。后端会保护"5 分钟内修改过"的文件（正在生成中的不能删）。
  const cleanTemp = async () => {
    try {
      const n = await invoke<number>("generator_cleanup_temp", { keep: [] });
      setMsg(`🗑 已清理 ${n} 个临时文件`);
      await loadTemp();
    } catch (e) { setMsg("❌ 清理失败：" + String(e)); }
  };

  /**
   * 第五批 ②：回滚第一步 —— **先看差异，再决定**。
   * 原实现是 confirm 一句"确定回滚吗"就直接整表替换，用户根本不知道会失去什么。
   */
  /** 第五批 ②：把临时设定版本转为永久（"这个版本我想留住"）。 */

  /** 第五批 ②：清掉临时设定版本（回滚前的自动备份）。程序启动时也会自动清。 */
  /** 回滚第二步：确认后执行（调用方已看过差异） */
  const rollbackTo = async (v: number) => {
    try {
      const r = await invoke<string>("rollback_snapshot", { storyId: String(currentStory?.id), version: v });
      setMsg("🔄 " + r);
      setSnapPreview(null);
      // 回滚前会自动存一个**临时**版本，刷新列表让它立刻可见（带 ⏳ 徽标）
      await loadSnapshots(); await loadAudit(); await loadRelations(currentStory!.id); await loadChars(currentStory!.id); await loadWorlds(currentStory!.id);
    } catch (e) { setMsg("❌ " + String(e)); }
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
    // 启动时就要判断"首次使用"——否则弹窗要等用户点进「合规与责任」才出现（实测踩过这个坑）
    void loadComplianceAck();
  }, []);

  const loadSettings = async () => {
    try {
      // 主模型 Key 存在系统凭据库里，界面查不到 —— 向后端问一个布尔值
      // （只回"有没有"，绝不回明文）
      invoke<{ has_key: boolean }>("main_model_key_status")
        .then((r) => setHasApiKey(!!r.has_key))
        .catch(() => setHasApiKey(false));
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
      const aa = await invoke<string | null>("get_config", { key: "evolution.auto_analyze" });
      setAutoAnalyze(aa !== "false");
      // 插件开关：逐一读取（有能力开关的插件才有配置键）
      const nextPlugins: Record<string, { enabled: boolean; panelOpen: boolean }> = {};
      for (const d of PLUGIN_REGISTRY) {
        if (!pluginCaps(d).canToggle) continue;
        const v = await invoke<string | null>("get_config", { key: pluginKey(d.id) });
        nextPlugins[d.id] = { enabled: v === "true", panelOpen: pluginState[d.id]?.panelOpen ?? false };
      }
      setPluginState((prev) => ({ ...prev, ...nextPlugins }));
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
      await invoke("set_config", { key: "evolution.auto_analyze", value: autoAnalyze ? "true" : "false" });

      if (apiKeyInput) {
        await invoke("set_api_key", { account: "openai_api_key", key: apiKeyInput });
        setHasApiKey(true);
        setApiKeyInput("");
      }
      setMsg("✅ 设置已保存");
    } catch (e) {
      setMsg("❌ 保存失败: " + String(e));
    }
  };

  /// 打开「🔍 回顾」弹窗时拉取可选的"天"

  const loadDebug = async () => {
    if (!currentStory) return;
    try {
      const list = await invoke<DebugReq[]>("list_debug_requests", { storyId: String(currentStory.id) });
      setDebugList(list);
    } catch (e) {
      setMsg("❌ 加载设定版本失败: " + String(e));
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

  // ===== 第五批 ③：分支 =====
  /** 读该故事的分支列表与"当前分支"（进入故事 / 切换后调用） */
  const loadBranches = async (storyId: number) => {
    try {
      const list = await invoke<BranchDto[]>("list_branches", { storyId: String(storyId) });
      setBranches(list);
      const cur = await invoke<string | null>("get_current_branch", { storyId: String(storyId) });
      // **防悬空引用**：记录的当前分支若已不在列表里（分支被删），一律按主线 ——
      // 否则界面会用一个不存在的分支去过滤，导致"看不到任何节点"甚至白屏（实测踩到）。
      const valid = cur != null && list.some((b) => b.id === cur) ? cur : null;
      if (cur != null && valid === null) console.warn("[织梦机] 当前分支已不存在，归主线：", cur);
      setCurrentBranchId(valid);
    } catch (e) { console.error("加载分支失败：", e); }
  };

  /** 切换分支：设定后端当前分支 → 重新加载全部相关数据（时间线/消息/角色/关系/世界观/版本/提案） */
  const switchBranch = async (branchId: string | null) => {
    if (!currentStory) return;
    // 后端记住"当前分支"（其它命令不传分支时按它过滤）
    const storyId = currentStory.id;
    setMsg("⏳ 正在切换分支…");
    try {
      await invoke("set_current_branch", { storyId: String(storyId), branchId });
      setCurrentBranchId(branchId);
      setFocusCharId(null);     // 角色 id 在分支间不同，清空右侧选中
      // **关键**：原来选中的节点若不属于新分支，必须清掉 ——
      // 否则它会一直是"当前节点"，于是时间维高亮别的分支的节点、新消息也写到那儿去
      // （用户实测："切回主线后时间维里还有分支的第 12 天，也能在它上面发消息"）。
      // 清成 null 后由 loadTimelineAndMessages 的默认逻辑挑"本分支最后一个节点"。
      setNode(null);
      await loadChars(storyId);
      await loadWorlds(storyId);
      await loadRelations(storyId);
      // 显式把目标分支传下去（本次渲染里 currentBranchId 还是旧值，不能依赖它做过滤）
      await loadTimelineAndMessages(storyId, undefined, branchId);
      await loadSnapshots();
      await loadProposals();
      const name = branchId === null ? "主线" : (branches.find((b) => b.id === branchId)?.name ?? "分支");
      setMsg("✅ 已切到「" + name + "」");
    } catch (e) { setMsg("❌ 切换分支失败: " + String(e)); }
  };

  /**
   * 删除分支（第五批 ③ 第 6 步）。
   *
   * 只删**这条分支自己的**节点与世界状态副本；共享历史（分叉点及其之前）与主线一律不动。
   * 后端会拒绝"还被别的分支依赖"的情况，并把原因返回给这里显示。
   */

  /** 第五批 ④：保存分支改名 / 备注。 */
  const saveBranchEdit = async () => {
    if (!currentStory || !branchEdit) return;
    const name = branchEdit.name.trim();
    if (!name) { setMsg("⚠ 分支名称不能为空"); return; }
    setBranchEdit({ ...branchEdit, busy: true });
    try {
      await invoke("rename_branch", {
        storyId: String(currentStory.id), branchId: branchEdit.id, name, note: branchEdit.note,
      });
      setBranchEdit(null);
      await loadBranches(currentStory.id);
      setMsg(`✅ 分支已更新为「${name}」`);
    } catch (e) {
      setBranchEdit((v) => (v ? { ...v, busy: false } : v));
      setMsg("❌ 保存失败：" + String(e));
    }
  };

  /** 打开"新建分支"弹窗（forkNodeId 为空时默认当前节点） */

  /** 创建分支：成功后自动切到新分支 */
  const createBranch = async () => {
    if (!currentStory) return;
    const name = branchDialog.name.trim();
    if (!name) { setMsg("⚠ 分支名称不能为空"); return; }
    if (branchDialog.forkNodeId == null) { setMsg("⚠ 请选择分叉节点"); return; }
    setBranchDialog((d) => ({ ...d, busy: true }));
    try {
      const b = await invoke<BranchDto>("create_branch", {
        storyId: String(currentStory.id), name, forkNodeId: branchDialog.forkNodeId,
      });
      setBranchDialog({ open: false, name: "", forkNodeId: null, busy: false });
      await loadBranches(currentStory.id);
      await switchBranch(b.id);   // 自动切到新分支
      setMsg(`✅ 分支「${b.name}」已创建，已切到该分支`);
    } catch (e) {
      setBranchDialog((d) => ({ ...d, busy: false }));
      setMsg("❌ 创建分支失败: " + String(e));
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
    await loadBranches(s.id);   // 第五批 ③：先取分支与当前分支（后端后续读取都按它过滤）
    await loadChars(s.id);
    await loadWorlds(s.id);
    await loadRelations(s.id);
    await loadTimelineAndMessages(s.id);
  };

  const loadTimelineAndMessages = async (storyId: number, nodeId?: number, branchOverride?: string | null) => {
    // branchOverride：切换分支时显式传入（此时 React state 还是旧值，不能拿它过滤）
    const effectiveBranch = branchOverride !== undefined ? branchOverride : currentBranchId;
    try {
      const nodes = await invoke<TimelineNodeDto[]>(
        "list_timeline_nodes",
        // 第五批 ③：**显式告诉后端"我在看哪条分支"**（不再依赖后端记住的状态）。
        // 后端只用它决定"默认落在哪个节点"，节点列表仍然返回全树（树状图要全）。
        { storyId: String(storyId), branchId: branchOverride !== undefined ? branchOverride : currentBranchId }
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
      // 第五批 ③：**默认节点必须落在当前分支自己的节点里**。
      // 原来取 `nodes[nodes.length - 1]`（按全局 sort_order 的最后一个）——
      // 切回主线时那个节点可能属于别的分支 → 时间维显示别的分支的节点、消息也写到那儿去了（实测踩到）。
      const mine = nodes.filter((n) => (n.branch_id ?? null) === (effectiveBranch ?? null));
      const candidates = mine.length > 0 ? mine : nodes;
      // 显式指定了 nodeId 时也要校验它属于当前分支（否则会把"别的分支的节点"设成当前节点）
      const wanted = nodeId != null ? nodes.find((n) => n.id === nodeId) : undefined;
      const target = (wanted && (wanted.branch_id ?? null) === (effectiveBranch ?? null))
        ? wanted
        : candidates[candidates.length - 1];
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



  const cancelEdit = () => {
    setEditingId(null);
    setEditText("");
  };


  // C 子项：在指定天数插入一个 day 节点（时间流逝区间里"插入事件"）
  const insertEventAtDay = async () => {
    if (!currentStory || !gapPrompt) return;
    const dayNo = Number(gapPrompt.day);
    if (!dayNo || dayNo <= 0) { setMsg("⚠ 请填写大于 0 的天数"); return; }
    // 与弹窗里的校验保持一致（按钮已禁用，这里兜一道防止别的入口绕过）
    if (dayNo < gapPrompt.from || dayNo > gapPrompt.to) {
      setMsg(`⚠ 只能填区间内的天数（第 ${gapPrompt.from} ~ ${gapPrompt.to} 天）`);
      return;
    }
    if (dayNodesSorted().some((x) => x.dayNo === dayNo)) {
      setMsg(`⚠ 第 ${dayNo} 天已经存在，不能重复创建`);
      return;
    }
    try {
      const n = await invoke<{ id: number; date_label: string }>("create_node_at_day", {
        storyId: String(currentStory.id), dayNo, branchId: currentBranchId,
      });
      setGapPrompt(null);
      setMsg(`✅ 已在${n.date_label}插入事件节点`);
      await loadTimelineAndMessages(currentStory.id, n.id);
    } catch (e) {
      setMsg("❌ 插入失败: " + String(e));
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


  /// 第五批（《现状说明》5.2）：**停止正在进行的生成**。
  /// 后端会在下一个 chunk 处退出，并把已生成的部分存下来（带"未完成"标记）。
  /// 清除已保存的主模型 Key（`delete_api_key` 之前没有 UI 入口，命令等于白注册）。
  const clearApiKey = async () => {
    if (!confirm("清除已保存的 API Key？\n\n只删 Windows 凭据管理器里的那一条，不影响其它数据。")) return;
    try {
      await invoke("delete_api_key", { account: "openai_api_key" });
      setHasApiKey(false);
      setApiKeyInput("");
      setMsg("🗑 已清除保存的 API Key");
    } catch (e) {
      setMsg("❌ 清除失败：" + String(e));
    }
  };


  // 第五批（《现状说明》5.3）：组件卸载时注销流式监听器（防泄漏）
  useEffect(() => () => {
    try { activeCleanupRef.current?.(); } catch { /* 忽略 */ }
    activeCleanupRef.current = null;
  }, []);

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
    // 换故事时重置"已备份"标记：新故事的设定还没备份过
    backupTakenRef.current = false;
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
  // ===== 第二批 · ② 检查 =====
  type CheckItemT = { where_: string; field: string; kind: string; why: string; suggestion: string };
  type PatchItemT = { board: string; target_kind: string; target_name: string; field: string; before?: string; value: string };
  const [checkBoard, setCheckBoard] = useState<"characters" | "world" | null>(null);
  const [checkBusy, setCheckBusy] = useState(false);
  const [checkItems, setCheckItems] = useState<CheckItemT[]>([]);
  const [checkPatch, setCheckPatch] = useState<PatchItemT[]>([]);
  const [checkPick, setCheckPick] = useState<Set<number>>(new Set());   // 一键替换：逐条勾选（默认全选）
  const [checkPrev, setCheckPrev] = useState("");       // 上一轮 AI 建议（只带这一份，不带多轮上下文）
  const [checkInput, setCheckInput] = useState("");     // 「继续对话」的用户输入
  const [checkNote, setCheckNote] = useState("");       // 失败 / 降级说明

  // 由字段归属推断名词（角色 / 关系 / 条目），供确认框与卡片共用
  const targetNounOf = (pp: PatchItemT) => {
    const RELF = ["relation_type", "strength", "state", "description"];
    const CHARF = ["name", "alias", "role", "gender", "age", "one_liner", "profile", "speech_style", "traits"];
    if (RELF.includes(pp.field) || pp.target_kind === "relation" || pp.target_name.includes("→")) return "关系";
    if (CHARF.includes(pp.field)) return "角色";
    return "条目";
  };

  // 二次确认对话框里用的字段名（与弹窗内保持一致；以 patch 的 board 为准）
  const fieldLabelOf = (pp: PatchItemT) => {
    const CHAR: Record<string, string> = {
      name: "角色名", alias: "别名", role: "定位", gender: "性别", age: "年龄",
      one_liner: "一句话身份", profile: "人物小传", speech_style: "说话方式", traits: "性格",
    };
    const REL: Record<string, string> = {
      relation_type: "关系类型", strength: "关系数值", state: "关系状态", description: "关系说明", direction: "关系方向",
    };
    const WORLD: Record<string, string> = {
      value: "内容", category: "分类", attitude: "对主角态度", scale: "规模", leader: "头目", control_range: "控制范围",
    };
    const owner = (pp.field in CHAR) ? "char" : (pp.field in REL) ? "rel" : (pp.field in WORLD) ? "world" : null;
    if (owner === "char") return CHAR[pp.field];
    if (owner === "rel") return REL[pp.field];
    if (owner === "world") return WORLD[pp.field];
    return `未知字段「${pp.field}」`;
  };

  // 组装板块内容（给 AI 读的原文；用 JSON 便于模型准确对应字段）
  const buildBoardContent = (board: "characters" | "world") => {
    if (board === "characters") {
      const cs = chars.map((c) => ({
        名称: c.name, 别名: c.alias || "", 定位: c.role || "", 性别: c.gender || "", 年龄: c.age || "",
        身份: c.one_liner || "", 人物小传: c.profile || "", 说话方式: c.speech_style || "", 性格: c.traits || "",
      }));
      const rs = relations.map((r) => {
        const f = chars.find((c) => c.id === r.from_character_id)?.name || `#${r.from_character_id}`;
        const tt = chars.find((c) => c.id === r.to_character_id)?.name || `#${r.to_character_id}`;
        return { 关系: `${f}→${tt}`, 关系类型: r.relation_type || "", 方向: r.direction || "", 数值: r.strength, 状态: r.state || "", 说明: r.description || "" };
      });
      return JSON.stringify({ 角色: cs, 关系: rs }, null, 1);
    }
    const ws = worlds.map((w) => ({
      名称: w.key, 分类: w.category || "", 内容: w.value || "",
      硬规则: w.is_hard_rule ? "是" : "否", 备注: w.meta_json || "",
    }));
    return JSON.stringify({ 世界设定: ws }, null, 1);
  };

  // 点「🔍 检查」：先跑本地规则（不花 token），再跑 AI（查语法/歧义/数值矛盾）
  const runCheck = async (board: "characters" | "world") => {
    if (!currentStory) return;
    setCheckBoard(board); setCheckBusy(true); setCheckItems([]); setCheckPatch([]);
    setCheckPrev(""); setCheckInput(""); setCheckNote(""); setCheckPick(new Set());
    const all: CheckItemT[] = [];
    try {
      const local = await invoke<{ items: CheckItemT[] }>("check_board_struct", { storyId: String(currentStory.id), board });
      all.push(...(local.items || []));
      setCheckItems([...all]);
    } catch (e) {
      setCheckNote("本地规则检查失败：" + String(e));
    }
    try {
      const rep = await invoke<{ items: CheckItemT[]; patch: PatchItemT[]; note?: string }>("check_board_ai", {
        storyId: String(currentStory.id), board,
        content: buildBoardContent(board), previous: "", userInput: "",
      });
      all.push(...(rep.items || []));
      setCheckItems([...all]);
      setCheckPatch(rep.patch || []);
      setCheckPick(new Set((rep.patch || []).map((_, i) => i)));
      setCheckPrev(JSON.stringify({ problems: rep.items || [], patch: rep.patch || [] }, null, 1));
      if (rep.note) setCheckNote(rep.note);
    } catch (e) {
      // AI 检查失败不影响本地结论；提示降级
      setCheckNote("AI 检查失败（仅显示本地规则结果）：" + String(e));
    }
    setCheckBusy(false);
  };

  // 「继续对话」：带 板块内容 + 上一轮建议 + 用户输入，让 AI 输出更新后的完整建议集
  const continueCheck = async () => {
    if (!currentStory || !checkBoard) return;
    setCheckBusy(true); setCheckNote("");
    try {
      const rep = await invoke<{ items: CheckItemT[]; patch: PatchItemT[]; note?: string }>("check_board_ai", {
        storyId: String(currentStory.id), board: checkBoard,
        content: buildBoardContent(checkBoard), previous: checkPrev, userInput: checkInput,
      });
      // 本轮 AI 结果替换上一轮 AI 部分：重新跑一次本地规则，保证结构问题不丢
      const local = await invoke<{ items: CheckItemT[] }>("check_board_struct", { storyId: String(currentStory.id), board: checkBoard });
      setCheckItems([...(local.items || []), ...(rep.items || [])]);
      setCheckPatch(rep.patch || []);
      setCheckPick(new Set((rep.patch || []).map((_, i) => i)));
      setCheckPrev(JSON.stringify({ problems: rep.items || [], patch: rep.patch || [] }, null, 1));
      setCheckInput("");
      if (rep.note) setCheckNote(rep.note);
    } catch (e) {
      setCheckNote("继续对话失败：" + String(e));
    }
    setCheckBusy(false);
  };

  // 「一键替换」：二次确认 → 白名单校验 → 执行 → 刷新
  const applyCheckPatch = async () => {
    if (!currentStory || checkPatch.length === 0) return;
    const picked = checkPatch.filter((_, i) => checkPick.has(i));
    if (picked.length === 0) { setMsg("⚠ 没有勾选任何条目"); return; }
    const lines = picked.map((p, i) => {
      const fl = fieldLabelOf(p);
      // 模型常把字段名写错（软件会改判）—— 写错时在确认框里明说，避免用户以为改的是别的地方
      const note = fl.startsWith("未知字段") ? "（字段名由软件按内容自动定位）" : "";
      return `${i + 1}. ${targetNounOf(p)}「${p.target_name}」· ${fl}${note}`;
    }).join("\n");
    if (!confirm(`将要修改 ${picked.length} 条：\n\n${lines}\n\n（执行前会自动存一个设定版本，可回滚）\n确定执行？`)) return;
    setCheckBusy(true);
    try {
      const done = await invoke<string[]>("apply_check_patch", { storyId: String(currentStory.id), items: picked });
      setMsg(`✅ 已替换 ${done.length} 条：${done.join("；")}`);
      setCheckBoard(null);
      await loadChars(currentStory.id);
      await loadWorlds(currentStory.id);
      await loadRelations(currentStory.id);
    } catch (e) {
      setMsg("❌ 替换失败（未做任何修改或已中止）：" + String(e));
    }
    setCheckBusy(false);
  };

  // ===== C 子项（时间流逝）=====
  // 天数节点按天数排序后，相邻跳号（如 5 → 8）之间就是一段"无事件的时间"。
  // **按当前分支过滤**：日历、时间流逝提示、阶段摘要都用它 → 只看自己这条线
  const dayNodesSorted = () =>
    timelineNodes
      .filter((n) => (n.branch_id ?? null) === (currentBranchId ?? null))
      .filter((n) => n.node_type !== "interlude" && n.node_type !== "flashback")
      .map((n) => ({ node: n, dayNo: parseInt(n.date_label.replace(/\D/g, ""), 10) }))
      .filter((x) => Number.isFinite(x.dayNo) && x.dayNo > 0)
      .sort((a, b) => a.dayNo - b.dayNo);

  const timeGaps = () => {
    const arr = dayNodesSorted();
    const out: { from: number; to: number }[] = [];
    for (let i = 0; i + 1 < arr.length; i++) {
      if (arr[i + 1].dayNo - arr[i].dayNo > 1) out.push({ from: arr[i].dayNo, to: arr[i + 1].dayNo });
    }
    return out;
  };

  // 区间条 / 时间维序列（日历下方与列表视图共用）
  const renderTimelineTrack = (opts?: { compact?: boolean }) => {
    // 第五批 ①：区间条渲染 **treeRows（全部节点，含插曲/倒叙）**，而不是只渲染天数节点。
    // 原因：加树状图时我把"时间线"横条删了做合并，而插曲**原本只在那条上显示**
    // （dayNodesSorted 按设计过滤掉插曲）→ 插曲在界面上彻底消失了（用户实测反馈）。
    // 现在区间条是唯一的时间显示，就必须把插曲也画出来。
    // "时间流逝"的空白期只在**相邻两个天数节点**之间插入，所以需要一个"上一个天数"的游标。
    let prevDay: { id: number; dayNo: number } | null = null;
    // **时间维只显示当前分支这一条线能看到的节点**（共享历史 + 本分支自己的）。
    // 别的分支的节点不出现 —— 原先"显示全树 + 打锁"会让用户以为数据错乱（实测反馈）。
    // 想看全部结构请用 🌲 时间线树状图。
    //
    // 第五批 ③ 第 6 步（树形化）：**其它分支收成一行"支线"** ——
    // 那一行显示分支名与该分支的节点数，点击可切过去。默认收起（带 ▸），
    // 用「树状图」里的折叠状态控制（这里只影响时间维这条）。
    const visibleRows = treeRowsFlat
      .filter((r) => {
        const nb = r.n.branch_id ?? null;
        // **这条线自己能看到的**（共享历史 ∪ 本分支自己的）→ 逐个显示
        // 注意：共享历史的节点 `branch_id` 是 NULL，但它属于**每条线的共同经历**，
        // 必须画出来；把"共享历史"和"别的分支"混为一谈会让第 1~11 天整段消失。
        if (branchPathIds.has(r.n.id)) return true;
        // 别的分支 → 只保留该分支的第一个节点作为代表行（渲染成"⑂ 支线"一行）
        const ownRows = treeRowsFlat.filter((x) => (x.n.branch_id ?? null) === nb);
        return ownRows.length > 0 && ownRows[0].n.id === r.n.id;
      });
    if (visibleRows.length === 0) return null;
    const items: ReactElement[] = [];
    visibleRows.forEach(({ n }) => {
      const dayNo = n.node_type === "interlude" || n.node_type === "flashback"
        ? NaN
        : parseInt(n.date_label.replace(/\D/g, ""), 10);
      // 天数节点之间跳号 → 先插一段"时间流逝"
      if (Number.isFinite(dayNo) && prevDay && dayNo - prevDay.dayNo > 1) {
        const from = prevDay.dayNo + 1, to = dayNo - 1;
        items.push(
          <button key={`gap-${prevDay.dayNo}-${dayNo}`}
            onClick={() => setGapPrompt({ from, to, day: String(from), title: "" })}
            title={`第 ${from}~${to} 天没有剧情（时间流逝）—— 点击可在这段里插入事件`}
            style={{
              fontSize: 10.5, padding: "2px 9px", borderRadius: 12, cursor: "pointer", fontFamily: "inherit", flexShrink: 0,
              border: `1px dashed ${T.line}`, background: "transparent", color: T.text3,
            }}>
            ⏳ 时间流逝 · 第 {from}~{to} 天（无事件）
          </button>
        );
      }
      const x = { node: n };
      const isCur = node?.id === x.node.id;
      // 第五批 ③：**节点属于别的分支时只读**（用户 2026-10-16 实测反馈）。
      // 树形展示需要看到其它分支的节点（否则看不出分叉），但**不能在这里操作它们** ——
      // 点它会切过去、删它会删错线，发消息也会写进别人分支。
      const nodeBranch = x.node.branch_id ?? null;
      const otherBranchName = nodeBranch === null ? "主线" : (branches.find((b) => b.id === nodeBranch)?.name ?? "其它分支");

      // **三件事要分清**（曾经混在一起，导致共享历史被吞掉，用户实测反馈）：
      //   ① `mineBranch`  —— 这个节点是"当前分支自己的"吗（**只有它能改**）
      //   ② `sharedHistory` —— 它在"当前这条线的父链上"吗（共享历史，**能看不能改**）
      //   ③ 其余 —— 别的分支，**收成一行"支线"**
      const mineBranch = nodeBranch === (currentBranchId ?? null);
      const inPath = branchPathIds.has(x.node.id);
      const sharedHistory = !mineBranch && inPath;

      // 渲染成"⑂ 支线"仅限③：既不是本分支的、也不在父链上
      if (!mineBranch && !sharedHistory && nodeBranch !== null) {
        const cnt = timelineNodes.filter((y) => (y.branch_id ?? null) === nodeBranch).length;
        items.push(
          <button key={`br-${nodeBranch}`}
            onClick={() => void switchBranch(nodeBranch)}
            title={`支线「${otherBranchName}」共 ${cnt} 个节点 —— 点击切到这条线`}
            style={{
              display: "inline-flex", alignItems: "center", gap: 6, flexShrink: 0,
              fontSize: 11, padding: "2px 10px", borderRadius: 12, cursor: "pointer", fontFamily: "inherit",
              border: `1px dashed ${branchColor(nodeBranch)}`, background: "transparent",
              color: branchColor(nodeBranch),
            }}>
            <span style={{ width: 8, height: 8, borderRadius: 4, background: branchColor(nodeBranch), flexShrink: 0 }} />
            ⑂ {otherBranchName}（{cnt}）
          </button>
        );
        // **不要碰 prevDay**：支线是"另一条线的代表行"，若在这里更新游标，
        // 后面同一条线的节点就会按分支的天数算空档 → 出现"时间流逝 · 第 2~11 天"
        // 这种把共享历史整段跳过的怪显示（用户实测反馈）。
        return;
      }

      // 当前分支的节点才推进"时间流逝"游标
      if (Number.isFinite(dayNo)) prevDay = { id: n.id, dayNo };

      items.push(
        // 第五批 ①：时间线条与"时间维"合并后，这一条同时承担
        // ① 显示时间维（含空白期）② 切换当前节点 ③ 删除节点
        // 第五批 ③：④ 标出节点属于哪条分支（别的分支只读）
        <span key={`d-${x.node.id}`} style={{ display: "inline-flex", alignItems: "center", flexShrink: 0 }}>
          <button
            onClick={() => { switchNode(x.node.id); setView("story"); }}
            title={`${x.node.date_label}${x.node.day_summary ? "（已有日摘要）" : ""} —— 点击切换到这${x.node.node_type === "interlude" ? "段插曲" : "天"}${sharedHistory ? "（共享历史，会带出分支之前的内容）" : ""}`}
            style={{
              fontSize: 11, padding: "2px 9px", borderRadius: "12px 0 0 12px",
              cursor: "pointer", fontFamily: "inherit",
              border: `1px solid ${isCur ? T.accent : T.line}`, borderRight: "none",
              background: isCur ? T.accentSoft : T.panel2,
              color: isCur ? T.accent : (sharedHistory ? T.text3 : T.text2),
              fontWeight: isCur ? 650 : 400,
              opacity: sharedHistory ? 0.75 : 1,
              fontStyle: x.node.node_type === "interlude" ? "italic" : "normal",
            }}>
            {sharedHistory && <span title="共享历史（分叉点及之前，各条线共同经历）" style={{ marginRight: 4, fontSize: 9 }}>◻</span>}
            {x.node.node_type === "interlude" ? "🎭 " : ""}{x.node.date_label}{x.node.day_summary ? " 📝" : ""}
          </button>
          <button
            onClick={(e) => {
              e.stopPropagation();
              if (!mineBranch) {
                setMsg(`⚠ 「${x.node.date_label}」${sharedHistory ? "是共享历史（分叉点及之前）" : `属于${otherBranchName}`} —— 请先切到${otherBranchName}再操作`);
                return;
              }
              removeNode(x.node.id, x.node.date_label);
            }}
            title={mineBranch ? `删除「${x.node.date_label}」节点及其所有消息` : `共享历史 / 其它分支的节点不能在这里删除`}
            style={{
              fontSize: 11, padding: "2px 7px", borderRadius: "0 12px 12px 0",
              cursor: mineBranch ? "pointer" : "not-allowed", fontFamily: "inherit",
              border: `1px solid ${isCur ? T.accent : T.line}`,
              background: isCur ? T.accentSoft : T.panel2, color: T.text3,
              opacity: mineBranch ? 1 : 0.35,
            }}>
            ×
          </button>
        </span>
      );
    });
    return (
      <div style={{ marginTop: opts?.compact ? 8 : 12 }}>
        
        <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 6 }}>{items}</div>
      </div>
    );
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
  // 可导出区间选项（day 节点）—— **按当前分支过滤**：导出的是"这一条线"的剧情
  const dayNodes = timelineNodes
    .filter((n) => (n.branch_id ?? null) === (currentBranchId ?? null))
    .filter((n) => n.node_type !== "interlude" && n.node_type !== "flashback");
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


  /**
   * 第五批 ②（B）：**首次直接编辑设定之前**自动存一个临时版本。
   *
   * 为什么需要：直接改角色卡 / 关系 / 世界观**不会**产生设定版本（只有"应用提案"才会），
   * 所以手滑改错了没有退路。有了这个，编辑前会先留一份 ⏳ 临时版本（退出程序即失效）。
   *
   * 设计取舍：
   * - **同一会话只存一次**（`backupTakenRef` 记住）—— 否则连续编辑十几个字段会存出十几个版本，
   *   把列表刷满，反而找不到真正的历史版本。
   * - 只标记"本次会话已备份"，**不做去重校验**：即使中间回滚过（那份备份可能已不适用），
   *   也不重复打扰；用户想要新备份可以随时用「回滚前自动备份」或「清理临时版本」重来。
   * - **失败不阻断保存**：备份存不上也要让用户能改设定。
   */
  const ensureEditBackup = async (what: string) => {
    if (!currentStory) return;
    if (backupTakenRef.current) return;
    backupTakenRef.current = true; // 先置位：即便失败也不再反复尝试（避免每次保存都卡一下）
    try {
      const v = await invoke<number>("make_edit_backup", {
        storyId: String(currentStory.id),
        what,
      });
      setMsg(`🛟 已在编辑前自动备份为临时版本 V${v}（退出程序即清除）`);
    } catch (e) {
      console.error("编辑前自动备份失败（忽略，不影响保存）：", e);
    }
  };










  // ===== 工作区视图 =====
  // ===== 生成参数页逻辑 =====
  const paramPortrait = portraits.find((p) => p.id === pParamId) ?? portraits.find((p) => p.story_id === currentStory?.id) ?? portraits[0] ?? null;
  // 第五批 ②：进入「演进提案」页时加载快照 / 审计 / 事件日志。
  //
  // **这是一处漏挂**：这三个 loader 原来只在"回滚成功后刷新"那一次被调用过，
  // 所以快照列表永远显示"暂无快照"（哪怕库里有 15 个），「回滚到此版本」按钮也就永远看不到
  // —— 用户实测反馈"到处都找不到回滚"，根因就在这里。
  // 切页 → 把中间工作区滚回顶部（否则页面顶部提示看不见）
  useEffect(() => {
    if (mainScrollRef.current) mainScrollRef.current.scrollTop = 0;
  }, [view]);

  useEffect(() => {
    if (view !== "proposals") return;
    // ⑤-B：读"是否必须先拍板"的开关（不依赖故事，放在前面）
    void loadRequireApproval();
    if (!currentStory) return;
    void loadSnapshots();
    void loadAudit();
    void loadEventLogs();
  }, [view, currentStory?.id]);
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
  const pendingGroups = new Set(proposals.filter((p) => p.status === "pending").map((p) => p.group_id)).size;
    // 第五批 ③ 第 5 步：新建分支弹窗（根层渲染，与其它弹窗一致）
    const branchDialogModal = branchDialog.open ? (
      <div style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,.64)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 950, padding: 20 }}>
        <div style={{ width: 460, maxWidth: "100%", background: T.win, border: `1px solid ${T.line}`, borderRadius: 12, padding: "16px 20px", boxShadow: "0 30px 80px rgba(0,0,0,.6)" }}>
          <div style={{ fontSize: 15, fontWeight: 650, marginBottom: 12 }}>新建故事分支</div>

          <div style={{ fontSize: 12, color: T.text3, marginBottom: 4 }}>分支名称</div>
          <input value={branchDialog.name} autoFocus
            onChange={(e) => setBranchDialog((d) => ({ ...d, name: e.target.value }))}
            onKeyDown={(e) => { if (e.key === "Enter") void createBranch(); }}
            placeholder="例如：角色A牺牲线"
            style={{ width: "100%", boxSizing: "border-box", padding: "7px 10px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontSize: 13, fontFamily: "inherit", marginBottom: 12 }} />

          <div style={{ fontSize: 12, color: T.text3, marginBottom: 4 }}>分叉节点（新分支从它的后面开始）</div>
          <select value={branchDialog.forkNodeId ?? ""}
            onChange={(e) => setBranchDialog((d) => ({ ...d, forkNodeId: Number(e.target.value) }))}
            style={{ width: "100%", boxSizing: "border-box", padding: "7px 10px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontSize: 13, fontFamily: "inherit", marginBottom: 12 }}>
            {timelineNodes.map((n) => (
              <option key={n.id} value={n.id}>
                {n.node_type === "interlude" ? "🎭 " : ""}{n.date_label}
                {n.branch_id ? "（分支内）" : ""}
              </option>
            ))}
          </select>

          <div style={{ fontSize: 11.5, color: T.text2, lineHeight: 1.8, border: `1px solid ${T.lineSoft}`, borderRadius: 8, padding: "9px 11px", marginBottom: 12 }}>
            将**复制当前分支的世界状态**到新分支：角色 {chars.length} · 关系 {relations.length} · 世界观 {worlds.length}
            <span style={{ color: T.text3 }}>
              <br />设定版本（永久）也会一并复制；临时版本不复制。
              <br />分叉点之前的对话节点是**共享**的，不复制。
            </span>
          </div>

          <div style={{ display: "flex", gap: 10, justifyContent: "flex-end" }}>
            <button onClick={() => setBranchDialog({ open: false, name: "", forkNodeId: null, busy: false })}
              style={{ fontSize: 12.5, padding: "7px 16px" }}>取消</button>
            <button disabled={branchDialog.busy || !branchDialog.name.trim() || branchDialog.forkNodeId == null}
              onClick={() => void createBranch()}
              style={{
                fontSize: 12.5, padding: "7px 18px", border: "none", borderRadius: 8, fontFamily: "inherit", fontWeight: 650,
                background: (branchDialog.busy || !branchDialog.name.trim()) ? T.panel3 : "linear-gradient(135deg,#FF7A18,#ff9a45)",
                color: (branchDialog.busy || !branchDialog.name.trim()) ? T.text3 : "#231505",
                cursor: (branchDialog.busy || !branchDialog.name.trim()) ? "not-allowed" : "pointer",
              }}>
              {branchDialog.busy ? "创建中…" : "创建并切换过去"}
            </button>
          </div>
        </div>
      </div>
    ) : null;

    // 第五批 ④：分支改名 / 备注弹窗（管理平行故事）
    const branchEditModal = branchEdit ? (
      <div style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,.64)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 950, padding: 20 }}>
        <div style={{ width: 460, maxWidth: "100%", background: T.win, border: `1px solid ${T.line}`, borderRadius: 12, padding: "16px 20px", boxShadow: "0 30px 80px rgba(0,0,0,.6)" }}>
          <div style={{ fontSize: 15, fontWeight: 650, marginBottom: 12 }}>分支改名 / 备注</div>

          <div style={{ fontSize: 12, color: T.text3, marginBottom: 4 }}>分支名称</div>
          <input value={branchEdit.name} autoFocus
            onChange={(e) => setBranchEdit({ ...branchEdit, name: e.target.value })}
            onKeyDown={(e) => { if (e.key === "Enter") void saveBranchEdit(); }}
            style={{ width: "100%", boxSizing: "border-box", padding: "7px 10px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontSize: 13, fontFamily: "inherit", marginBottom: 12 }} />

          <div style={{ fontSize: 12, color: T.text3, marginBottom: 4 }}>备注（一句话说明这条线是干什么的，可留空）</div>
          <textarea value={branchEdit.note} rows={3}
            onChange={(e) => setBranchEdit({ ...branchEdit, note: e.target.value })}
            placeholder="例如：假设夏婉琴在第 11 天没有回去，剧情会怎么走"
            style={{ width: "100%", boxSizing: "border-box", padding: "7px 10px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontSize: 13, fontFamily: "inherit", resize: "vertical", marginBottom: 12 }} />

          <div style={{ fontSize: 11.5, color: T.text3, lineHeight: 1.8, border: `1px solid ${T.lineSoft}`, borderRadius: 8, padding: "9px 11px", marginBottom: 12 }}>
            备注只在分支条与 🌲 树状图里显示（鼠标悬停可见），不会进入 AI 提示词。
          </div>

          <div style={{ display: "flex", gap: 10, justifyContent: "flex-end" }}>
            <button onClick={() => setBranchEdit(null)} style={{ fontSize: 12.5, padding: "7px 16px" }}>取消</button>
            <button disabled={branchEdit.busy || !branchEdit.name.trim()} onClick={() => void saveBranchEdit()}
              style={{
                fontSize: 12.5, padding: "7px 18px", border: "none", borderRadius: 8, fontFamily: "inherit", fontWeight: 650,
                background: (branchEdit.busy || !branchEdit.name.trim()) ? T.panel3 : "linear-gradient(135deg,#FF7A18,#ff9a45)",
                color: (branchEdit.busy || !branchEdit.name.trim()) ? T.text3 : "#231505",
                cursor: (branchEdit.busy || !branchEdit.name.trim()) ? "not-allowed" : "pointer",
              }}>
              {branchEdit.busy ? "保存中…" : "保存"}
            </button>
          </div>
        </div>
      </div>
    ) : null;


    // 第五批 ②：回滚差异预览弹窗。同样渲染在根层（与合规弹窗一致，避免被容器的 overflow 裁剪）。
    const snapPreviewModal = snapPreview ? (
      <div style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,.66)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 900, padding: 20 }}>
        <div style={{ width: 720, maxWidth: "100%", maxHeight: "86vh", overflowY: "auto", background: T.win, border: `1px solid ${T.line}`, borderRadius: 12, padding: "16px 20px", boxShadow: "0 30px 80px rgba(0,0,0,.6)" }}>
          <div style={{ fontSize: 15, fontWeight: 650, marginBottom: 4 }}>
            ⚠ 回滚到设定版本 V{snapPreview.version} —— 先看清会发生什么
          </div>
          <div style={{ fontSize: 11.5, color: T.text3, lineHeight: 1.75, marginBottom: 12 }}>
            回滚会用该**设定版本**整表替换当前的角色 / 关系 / 世界观。<b style={{ color: T.warn }}>这是不可撤销的</b>（回滚本身也会存成一个新的设定版本）。
            下面列出「当前 → 回滚后」的差异：
          </div>

          {snapPreview.loading && <div style={{ fontSize: 12.5, color: T.text3, padding: "18px 0" }}>正在对比…</div>}
          {!!snapPreview.error && (
            <div style={{ fontSize: 12.5, color: T.danger, padding: "10px 12px", border: `1px solid ${T.danger}`, borderRadius: 8, marginBottom: 12 }}>
              对比失败：{snapPreview.error}
              <div style={{ fontSize: 11, color: T.text3, marginTop: 6 }}>仍然可以回滚，但建议先查清原因。</div>
            </div>
          )}

          {!snapPreview.loading && (() => {
            const d = snapPreview.diff;
            const total = d.characters.length + d.relations.length + d.world_settings.length;
            const KIND: Record<string, { t: string; c: string }> = {
              added: { t: "将新增", c: T.ok },
              removed: { t: "将删除", c: T.danger },
              modified: { t: "将修改", c: T.warn },
            };
            const Section = ({ title, items }: { title: string; items: SnapItemChange[] }) => items.length === 0 ? null : (
              <div style={{ marginBottom: 14 }}>
                <div style={{ fontSize: 12.5, fontWeight: 650, marginBottom: 6 }}>{title}（{items.length}）</div>
                <div style={{ display: "flex", flexDirection: "column", gap: 5 }}>
                  {items.map((it, i) => (
                    <div key={i} style={{ border: `1px solid ${T.lineSoft}`, borderRadius: 8, padding: "7px 10px", background: T.panel }}>
                      <div style={{ fontSize: 12 }}>
                        <span style={{ color: KIND[it.kind]?.c ?? T.text2, fontWeight: 650 }}>{KIND[it.kind]?.t ?? it.kind}</span>
                        <span style={{ marginLeft: 8, color: T.text1 }}>{it.label}</span>
                      </div>
                      {it.fields.length > 0 && (
                        <div style={{ marginTop: 5, paddingLeft: 2 }}>
                          {it.fields.map((f, j) => (
                            <div key={j} style={{ fontSize: 11, color: T.text3, lineHeight: 1.75 }}>
                              {f.field}：
                              <span style={{ color: T.text2 }}>{f.before || "（空）"}</span>
                              <span style={{ margin: "0 6px", color: T.accent }}>→</span>
                              <span style={{ color: T.text1 }}>{f.after || "（空）"}</span>
                            </div>
                          ))}
                        </div>
                      )}
                    </div>
                  ))}
                </div>
              </div>
            );
            if (total === 0) {
              return (
                <div style={{ fontSize: 12.5, color: T.text3, border: `1px dashed ${T.line}`, borderRadius: 8, padding: "14px 12px", marginBottom: 12 }}>
                  当前设定与该版本**完全一致**，回滚不会产生任何变化。
                </div>
              );
            }
            return (
              <>
                <div style={{ fontSize: 12, color: T.text2, marginBottom: 10 }}>
                  共 <b style={{ color: T.accent }}>{total}</b> 处不同：
                  新增 {[...d.characters, ...d.relations, ...d.world_settings].filter((x) => x.kind === "added").length} ·
                  修改 {[...d.characters, ...d.relations, ...d.world_settings].filter((x) => x.kind === "modified").length} ·
                  删除 {[...d.characters, ...d.relations, ...d.world_settings].filter((x) => x.kind === "removed").length}
                </div>
                <Section title="角色卡" items={d.characters} />
                <Section title="关系" items={d.relations} />
                <Section title="世界观 / 设定" items={d.world_settings} />
              </>
            );
          })()}

          <div style={{ display: "flex", gap: 10, justifyContent: "flex-end", marginTop: 16 }}>
            <button onClick={() => setSnapPreview(null)} style={{ fontSize: 12.5, padding: "7px 16px" }}>取消</button>
            <button
              disabled={snapPreview.loading || (() => { const d = snapPreview.diff; return d.characters.length + d.relations.length + d.world_settings.length === 0; })()}
              onClick={() => void rollbackTo(snapPreview.version)}
              title="整表替换设定，不可撤销"
              style={{
                fontSize: 12.5, padding: "7px 18px", border: "none", borderRadius: 8, fontFamily: "inherit",
                background: (() => { const d = snapPreview.diff; return d.characters.length + d.relations.length + d.world_settings.length === 0; })() ? T.panel3 : T.danger,
                color: (() => { const d = snapPreview.diff; return d.characters.length + d.relations.length + d.world_settings.length === 0; })() ? T.text3 : "#fff",
                fontWeight: 650,
                cursor: (() => { const d = snapPreview.diff; return d.characters.length + d.relations.length + d.world_settings.length === 0; })() ? "not-allowed" : "pointer",
              }}>
              确认回滚（{(() => { const d = snapPreview.diff; return d.characters.length + d.relations.length + d.world_settings.length; })()} 处变更）
            </button>
          </div>
        </div>
      </div>
    ) : null;

    // 首次启动的合规弹窗：**必须在组件根层渲染**。
    // 踩过的坑：起初放在主容器（overflow: hidden）内部，position: fixed 被祖先裁掉，
    // 只显示成页面一小块、看起来像首页多了一块东西。现在作为最外层容器的兄弟节点渲染。
    const complianceModal = (
      complianceNotice ? (
        // 首次启动的**强制**确认：必须勾选才能继续（点遮罩不关闭、按 Esc 不关闭）
        <div style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,.72)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 1000, padding: 20 }}>
          <div style={{ width: 640, maxWidth: "100%", maxHeight: "86vh", overflowY: "auto", background: T.win, border: `1px solid ${T.line}`, borderRadius: 12, padding: "18px 20px", boxShadow: "0 30px 80px rgba(0,0,0,.6)" }}>
            <div style={{ fontSize: 16, fontWeight: 650, marginBottom: 4 }}>⚖ 使用前必读</div>
            <div style={{ fontSize: 11, color: T.text3, marginBottom: 12 }}>
              首次使用请确认以下内容。本弹窗需勾选后才能继续（确认一次即可，之后不再出现）。
            </div>
            {/* 协议与免责摘要 */}
            <div style={{ border: `1px solid ${T.accent}`, borderRadius: 8, background: T.accentSoft, padding: "10px 12px", marginBottom: 12 }}>
              <div style={{ fontSize: 12, fontWeight: 650, color: T.accent, marginBottom: 5 }}>📜 开源协议与免责声明</div>
              {LICENSE_NOTICE.lines.map((x, i) => (
                <div key={i} style={{ fontSize: 11.5, color: T.text2, lineHeight: 1.75 }}>· {renderBold(x)}</div>
              ))}
            </div>
            <div style={{ fontSize: 12.5, color: T.text2, lineHeight: 1.9 }}>
              欢迎使用本软件。它只是运行在你本机的工具，有几件事先说清楚：
              <div style={{ marginTop: 10, paddingLeft: 4 }}>
                · 它<b>不提供</b>生成式 AI 服务，也<b>不代你申请</b>任何接口；调用的是<b>你自己配置</b>的模型与绘图服务
                <br />· 它<b>不上传</b>你的数据：故事、角色、素材都留在本机
                <br />· 它<b>不会</b>帮你抹掉 AI 标识；反过来，绘图功能默认会在图上写入「AI」角标与元数据标识
                <br />· AI 生成内容的<b>合规义务在使用者</b>：对外发布请显式标注「AI 生成」，不要删除或篡改标识
                <br />· 不要用真人姓名或照片生成其形象；不要生成违法内容
                <br />· 不要生成损害心理健康的内容（自伤、暴力、极端仇恨、成瘾诱导等）；不诱导 AI 绕过服务方安全过滤
              </div>
              {/* 用户指定必须包含的两句（措辞与 README / LICENSE 保持一致） */}
              <div style={{ marginTop: 10, padding: "9px 11px", borderRadius: 8, border: `1px solid ${T.danger}`, background: "rgba(240,85,77,.08)", fontSize: 12, color: T.text1, lineHeight: 1.85, fontWeight: 600 }}>
                {REQUIRED_NOTICES.map((x, i) => (<div key={i}>{i + 1}. {x}</div>))}
              </div>
              <div style={{ marginTop: 12, fontSize: 11.5, color: T.text3, lineHeight: 1.8 }}>
                依据：《人工智能生成合成内容标识办法》（2025-09-01 施行）、《生成式人工智能服务管理暂行办法》等。
                完整说明与场景指引见左侧导航「⚖ 合规与责任」；本提示为工程整理，不构成法律意见。
              </div>
            </div>
            {/* 强制勾选：不勾不能进 */}
            <label style={{ display: "flex", alignItems: "flex-start", gap: 8, marginTop: 14, cursor: "pointer",
              border: `1px solid ${noticeAgreed ? T.ok : T.line}`, borderRadius: 8, padding: "10px 12px", background: noticeAgreed ? "rgba(55,201,120,.08)" : "transparent" }}>
              <input type="checkbox" checked={noticeAgreed} onChange={(e) => setNoticeAgreed(e.target.checked)}
                style={{ marginTop: 3, cursor: "pointer", accentColor: T.accent }} />
              <span style={{ fontSize: 12, color: T.text2, lineHeight: 1.75 }}>
                我已阅读并同意上述开源协议与免责声明；我理解 <b>AI 生成内容的标识与合规义务由我（使用者）承担</b>，
                不会删除或篡改 AI 标识，也不会用本软件生成违法内容或未经授权使用他人肖像。
              </span>
            </label>

            <div style={{ display: "flex", gap: 10, justifyContent: "space-between", alignItems: "center", marginTop: 14, flexWrap: "wrap" }}>
              {/* 刻意只留一个出口：没有"先去别处看看"这类按钮。
                  曾经有一个「去读完整说明」，它会关掉弹窗并跳页 —— 等于给强制确认开了后门（用户实测发现）。
                  现在不勾选就无法离开弹窗；完整说明在确认后可从左侧导航「合规与责任」随时查阅。 */}
              <span style={{ fontSize: 11, color: T.text3 }}>
                勾选后即可开始使用；完整说明在左侧导航「⚖ 合规与责任」，随时可查。
                <br />条款版本 <b style={{ color: T.text2 }}>{NOTICE_VERSION}</b>
                <span>（条款内容更新后需重新确认一次）</span>
              </span>
              <button disabled={!noticeAgreed}
                onClick={() => { if (noticeAgreed) void dismissComplianceNotice(); }}
                title={noticeAgreed ? "" : "请先勾选上方确认项"}
                style={{ fontSize: 12.5, padding: "7px 18px", border: "none", borderRadius: 8,
                  background: noticeAgreed ? "linear-gradient(135deg,#FF7A18,#ff9a45)" : T.panel3,
                  color: noticeAgreed ? "#231505" : T.text3,
                  fontWeight: 650, cursor: noticeAgreed ? "pointer" : "not-allowed", fontFamily: "inherit" }}>
                {noticeAgreed ? "同意并开始使用" : "请先勾选确认"}
              </button>
            </div>
          </div>
        </div>
      ) : null
    );

    return (
      <>
      <div style={{ display: "flex", height: "100vh", fontFamily: "system-ui, sans-serif", background: T.win, color: T.text1 }}>
          <style>{`::-webkit-scrollbar{width:8px;height:8px}::-webkit-scrollbar-track{background:transparent}::-webkit-scrollbar-thumb{background:rgba(255,255,255,.14);border-radius:4px}::-webkit-scrollbar-thumb:hover{background:rgba(255,255,255,.24)}@keyframes dsh-blink{0%,49%{opacity:1}50%,100%{opacity:0}}button{background:#191c22;color:#a3a9b8;border:1px solid #2e333d;border-radius:6px;font-family:inherit;cursor:pointer}button:hover{color:#fff;border-color:#3d4451}button:disabled{opacity:.5;cursor:not-allowed}input,select,textarea{background:#191c22;color:#e8eaf0;border:1px solid #2e333d;border-radius:6px;font-family:inherit}input:focus,select:focus,textarea:focus{border-color:rgba(255,122,24,.55);outline:none}`}</style>
        {/* ① 左导航 */}
        <Sidebar
          complianceAck={complianceAck}
          currentStory={currentStory}
          loadCloudConfig={loadCloudConfig}
          loadComplianceAck={loadComplianceAck}
          loadDebug={loadDebug}
          loadEvals={loadEvals}
          loadGenerators={loadGenerators}
          navItems={navItems}
          pendingGroups={pendingGroups}
          setGenProbe={setGenProbe}
          setShowSettings={setShowSettings}
          setView={setView}
          view={view}
        />

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

          <div ref={mainScrollRef} style={{ flex: 1, overflowY: "auto", padding: "20px 24px 12px" }}>
            {msg && <p style={{ fontSize: 13, color: T.text2, marginTop: 0 }}>{msg}</p>}

            {/* 故事列表（仅点击左侧"故事列表"时显示；首页为空白）
                宽度自适应：不再限宽，随中间工作区撑满（左右留白各 24px，来自上一层容器 padding） */}
        {view === "stories" && (
          <StoriesPage
            createStory={createStory}
            enterStory={enterStory}
            loadStories={loadStories}
            newOrigin={newOrigin}
            newTitle={newTitle}
            removeStory={removeStory}
            setNewOrigin={setNewOrigin}
            setNewTitle={setNewTitle}
            setShowNew={setShowNew}
            showNew={showNew}
            stories={stories}
          />
        )}



        {view === "chars" && (
          <CharsPage
            charGraphRootId={charGraphRootId}
            chars={chars}
            charsLoading={charsLoading}
            charUsage={charUsage}
            currentStory={currentStory}
            ensureEditBackup={ensureEditBackup}
            loadChars={loadChars}
            loadRelations={loadRelations}
            openGenPanel={openGenPanel}
            portraits={portraits}
            relations={relations}
            runCheck={runCheck}
            setCharGraphRootId={setCharGraphRootId}
            setMsg={setMsg}
            setView={setView}
          />
        )}
        {view === "world" && (
          <WorldPage
            currentStory={currentStory}
            ensureEditBackup={ensureEditBackup}
            loadWorlds={loadWorlds}
            runCheck={runCheck}
            setMsg={setMsg}
            worlds={worlds}
          />
        )}
        {view === "proposals" && (
          <ProposalsPage
            setSnapPreview={setSnapPreview}
            analyzeInfo={analyzeInfo}
            auditLogs={auditLogs}
            chars={chars}
            currentStory={currentStory}
            eventLogs={eventLogs}
            loadAudit={loadAudit}
            loadChars={loadChars}
            loadProposals={loadProposals}
            loadRelations={loadRelations}
            loadSnapshots={loadSnapshots}
            node={node}
            proposals={proposals}
            relations={relations}
            requireApproval={requireApproval}
            selGroups={selGroups}
            setAnalyzeInfo={setAnalyzeInfo}
            setMsg={setMsg}
            setRequireApproval={setRequireApproval}
            setSelGroups={setSelGroups}
            snapshots={snapshots}
            timelineNodes={timelineNodes}
          />
        )}

        {view === "temp" && (
          <TempPage tempFiles={tempFiles} loadTemp={loadTemp} cleanTemp={cleanTemp} />
        )}

        {view === "timeline" && (
          <TimelinePage
            forceTreeRedraw={forceTreeRedraw}
            branchColor={branchColor}
            branchLabelOf={branchLabelOf}
            branchScopedNodes={branchScopedNodes}
            dayNodesSorted={dayNodesSorted}
            nodeDateDisplay={nodeDateDisplay}
            removeNode={removeNode}
            renderTimelineTrack={renderTimelineTrack}
            switchNode={switchNode}
            timeGaps={timeGaps}
            treeCollapsedRef={treeCollapsedRef}
            treeRows={treeRows}
            setCalOffset={setCalOffset}
            setGapPrompt={setGapPrompt}
            setTimelineView={setTimelineView}
            setView={setView}
            branches={branches}
            calOffset={calOffset}
            currentBranchId={currentBranchId}
            currentStory={currentStory}
            node={node}
            timelineNodes={timelineNodes}
            timelineView={timelineView}
          />
        )}

        {view === "portrait" && (
          <PortraitPage
            pDetail={pDetail}
            setPDetail={setPDetail}
            portraits={portraits}
            loadPortraits={loadPortraits}
            setMsg={setMsg}
            setView={setView}
            stories={stories}
            openGenPanel={openGenPanel}
            loadGenerators={loadGenerators}
            setPluginFlag={setPluginFlag}
          />
        )}
        {view === "params" && (
          <ParamsPage
            ctxWindow={ctxWindow}
            currentStory={currentStory}
            debugList={debugList}
            loadDebug={loadDebug}
            loadPortraits={loadPortraits}
            modelName={modelName}
            nRecentInput={nRecentInput}
            node={node}
            pDebugId={pDebugId}
            pParamForm={pParamForm}
            paramPortrait={paramPortrait}
            portraits={portraits}
            setDebugList={setDebugList}
            setMsg={setMsg}
            setPDebugId={setPDebugId}
            setPParamForm={setPParamForm}
            setPParamId={setPParamId}
            setSelectedDebug={setSelectedDebug}
            setView={setView}
            timelineNodes={timelineNodes}
          />
        )}
        {view === "plugins" && (
          <PluginsPage
            genPrefill={genPrefill}
            chars={chars}
            cloudForm={cloudForm}
            cloudOverrides={cloudOverrides}
            cloudSaving={cloudSaving}
            currentStory={currentStory}
            evalBusy={evalBusy}
            evals={evals}
            genBusy={genBusy}
            genList={genList}
            genProbe={genProbe}
            genProbing={genProbing}
            genSel={genSel}
            genSummary={genSummary}
            genUrl={genUrl}
            loadCloudConfig={loadCloudConfig}
            loadEvals={loadEvals}
            node={node}
            pluginKey={pluginKey}
            pluginState={pluginState}
            runGenerate={runGenerate}
            setCloudForm={setCloudForm}
            setCloudSaving={setCloudSaving}
            setEvalBusy={setEvalBusy}
            setGenProbe={setGenProbe}
            setGenProbing={setGenProbing}
            setGenSel={setGenSel}
            setGenUrl={setGenUrl}
            setMsg={setMsg}
            setPluginFlag={setPluginFlag}
          />
        )}

        {view === "compliance" && (
          <CompliancePage
            complianceAck={complianceAck}
            ackCompliance={ackCompliance}
            checklistDone={checklistDone}
            setChecklistDone={setChecklistDone}
          />
        )}

        {view === "story" && (
          <StoryPanel
            branches={branches}
            cancelEdit={cancelEdit}
            chars={chars}
            currentBranchId={currentBranchId}
            currentStory={currentStory}
            dayNodesSorted={dayNodesSorted}
            editText={editText}
            editingId={editingId}
            loadBranches={loadBranches}
            loadChars={loadChars}
            loadRelations={loadRelations}
            loadTimelineAndMessages={loadTimelineAndMessages}
            loadWorlds={loadWorlds}
            messages={messages}
            node={node}
            renderTimelineTrack={renderTimelineTrack}
            setBranchDialog={setBranchDialog}
            setBranchEdit={setBranchEdit}
            setCurrentBranchId={setCurrentBranchId}
            setEditText={setEditText}
            setEditingId={setEditingId}
            setMsg={setMsg}
            switchBranch={switchBranch}
            timeGaps={timeGaps}
            timelineNodes={timelineNodes}
            timelineView={timelineView}
          />
        )}
          </div>

          {/* 第二批 · ② 检查：报告弹窗（没问题点任意处关 / 有问题逐条列 + 一键替换·取消·继续对话） */}
          {checkBoard && (() => {
            const noIssue = !checkBusy && checkItems.length === 0 && checkPatch.length === 0;
            // 字段名映射按板块区分（避免角色板块的补丁显示成「规模 / 对主角态度」这类世界字段名）
            const CHAR_FIELDS: Record<string, string> = {
              name: "角色名", alias: "别名", role: "定位", gender: "性别", age: "年龄", one_liner: "一句话身份",
              profile: "人物小传", speech_style: "说话方式", traits: "性格",
            };
            const REL_FIELDS: Record<string, string> = {
              relation_type: "关系类型", strength: "关系数值", state: "关系状态", description: "关系说明", direction: "关系方向",
            };
            const WORLD_FIELDS: Record<string, string> = {
              key: "名称", value: "内容", category: "分类", attitude: "对主角态度", scale: "规模",
              leader: "头目", control_range: "控制范围", map_x: "地图 X", map_y: "地图 Y",
            };
            // 映射表以 **patch 自带的 board** 为准（不靠 target_kind 猜，避免模型标错类型时显示成别的板块字段）
            // 字段名与"角色/关系/条目"后缀**由字段归属直接推断** —— 这样即使模型把 board / target_kind 写错，
            // 界面也不会显示成别的板块的字段（此前出现过角色补丁显示成「条目 · 内容」的情况）。
            const fieldOwned = (f: string): "char" | "rel" | "world" | null =>
              (f in CHAR_FIELDS) ? "char" : (f in REL_FIELDS) ? "rel" : (f in WORLD_FIELDS) ? "world" : null;
            const fieldLabel = (pp: { board?: string; target_kind: string; field: string }) => {
              const owner = fieldOwned(pp.field);
              if (owner === "char") return CHAR_FIELDS[pp.field];
              if (owner === "rel") return REL_FIELDS[pp.field];
              if (owner === "world") return WORLD_FIELDS[pp.field];
              return `未知字段「${pp.field}」`;
            };
            const targetNoun = (pp: { target_kind: string; field: string }) =>
              fieldOwned(pp.field) === "rel" ? "关系" : fieldOwned(pp.field) === "char" ? "角色" : "条目";
            const kindColor = (k: string) => k === "空值" ? T.warn : k === "重复" || k === "悬空" || k === "坐标冲突" || k === "数值非法" ? T.danger : "#8ab4ff";
            const before = (p: { target_kind: string; target_name: string; field: string }) => {
              try {
                if (p.target_kind === "character") {
                  const c = chars.find((x) => x.name === p.target_name);
                  return c ? String((c as unknown as Record<string, string>)[p.field] ?? "") : "（找不到该角色）";
                }
                if (p.target_kind === "relation") {
                  const [f, tt] = p.target_name.split(/→|->/).map((s) => s.trim());
                  const a = chars.find((x) => x.name === f), b = chars.find((x) => x.name === tt);
                  const r = a && b ? relations.find((x) => x.from_character_id === a.id && x.to_character_id === b.id) : undefined;
                  return r ? String((r as unknown as Record<string, string>)[p.field] ?? "") : "（找不到该关系）";
                }
                const w = worlds.find((x) => x.key === p.target_name);
                return w ? String((w as unknown as Record<string, string>)[p.field] ?? "") : "（找不到该条目）";
              } catch { return ""; }
            };
            const cut = (s: string, n = 90) => s.length > n ? s.slice(0, n) + "…" : s;
            return (
              <div style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,.55)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 61 }}
                onClick={() => { if (noIssue) setCheckBoard(null); }}>
                <div onClick={(e) => e.stopPropagation()} style={{ width: noIssue ? 360 : 720, maxWidth: "92vw", maxHeight: "86vh", overflowY: "auto", background: T.panel, border: `1px solid ${T.line}`, borderRadius: 14, padding: "18px 20px" }}>
                  <div style={{ fontSize: 14, fontWeight: 650, marginBottom: 4 }}>
                    🔍 检查 · {checkBoard === "characters" ? "角色管理" : "世界设定"}
                  </div>

                  {/* 1) 没问题：点任意处关闭 */}
                  {noIssue ? (
                    <div style={{ textAlign: "center", padding: "18px 0 6px" }}>
                      <div style={{ fontSize: 30, marginBottom: 8 }}>✅</div>
                      <div style={{ fontSize: 13, color: T.ok, fontWeight: 650 }}>没发现问题</div>
                      <div style={{ fontSize: 11.5, color: T.text3, marginTop: 6, lineHeight: 1.8 }}>
                        本地规则与 AI 都没挑出读不通、有歧义或与数据字典矛盾的地方。<br />（点击任意处关闭）
                      </div>
                    </div>
                  ) : (
                    <>
                      {/* 2) 检查中 */}
                      {checkBusy && (
                        <div style={{ fontSize: 12, color: T.text3, padding: "10px 0" }}>正在检查…（本地规则即时出结果；AI 检查需几秒）</div>
                      )}

                      {/* 3) 问题清单 */}
                      {checkItems.length > 0 && (
                        <div style={{ border: `1px solid ${T.line}`, borderRadius: 10, overflow: "hidden", marginTop: 8 }}>
                          <div style={{ display: "grid", gridTemplateColumns: "150px 1fr", gap: 0, fontSize: 10.5, color: T.text3, background: T.panel2, padding: "7px 12px" }}>
                            <span>位置 / 字段</span><span>问题与建议</span>
                          </div>
                          {checkItems.map((it, i) => (
                            <div key={i} style={{ display: "grid", gridTemplateColumns: "150px 1fr", gap: 0, borderTop: `1px solid ${T.lineSoft}`, padding: "9px 12px", fontSize: 11.5 }}>
                              <div>
                                <div style={{ color: T.text1, fontWeight: 600, overflow: "hidden", textOverflow: "ellipsis" }}>{it.where_ || "（未指明）"}</div>
                                <div style={{ color: T.text3, fontSize: 10.5, marginTop: 2 }}>{it.field || "—"}</div>
                                <div style={{ display: "inline-block", marginTop: 4, fontSize: 9.5, padding: "1px 6px", borderRadius: 5, color: kindColor(it.kind), background: `${kindColor(it.kind)}22` }}>{it.kind || "问题"}</div>
                              </div>
                              <div style={{ color: T.text2, lineHeight: 1.7 }}>
                                <div>{it.why}</div>
                                {it.suggestion && <div style={{ color: T.ok, marginTop: 3 }}>建议：{it.suggestion}</div>}
                              </div>
                            </div>
                          ))}
                        </div>
                      )}

                      {/* 4) 可执行的补丁（AI 给出的结构化修改） */}
                      {checkPatch.length > 0 && (
                        <div style={{ marginTop: 12 }}>
                          <div style={{ fontSize: 12, fontWeight: 650, marginBottom: 6, display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
                            <span>可一键替换 {checkPatch.length} 条（已勾选 {checkPick.size} 条；执行前会二次确认并存一个设定版本）</span>
                            <button onClick={() => setCheckPick(new Set(checkPatch.map((_, i) => i)))}
                              style={{ fontSize: 10.5, padding: "2px 9px", borderRadius: 6, border: `1px solid ${T.line}`, background: "transparent", color: T.text3, cursor: "pointer" }}>全选</button>
                            <button onClick={() => setCheckPick(new Set())}
                              style={{ fontSize: 10.5, padding: "2px 9px", borderRadius: 6, border: `1px solid ${T.line}`, background: "transparent", color: T.text3, cursor: "pointer" }}>全不选</button>
                          </div>
                          <div style={{ border: `1px solid ${T.line}`, borderRadius: 10, overflow: "hidden" }}>
                            {checkPatch.map((pp, i) => (
                              <div key={i} style={{ borderTop: i ? `1px solid ${T.lineSoft}` : "none", padding: "9px 12px", fontSize: 11, opacity: checkPick.has(i) ? 1 : 0.45, display: "flex", gap: 9, alignItems: "flex-start" }}>
                                <input type="checkbox" checked={checkPick.has(i)}
                                  onChange={(e) => setCheckPick((prev) => { const nx = new Set(prev); if (e.target.checked) nx.add(i); else nx.delete(i); return nx; })}
                                  style={{ marginTop: 2, cursor: "pointer", accentColor: T.accent }} />
                                <div style={{ flex: 1, minWidth: 0 }}>
                                <div style={{ color: T.text1, fontWeight: 600 }}>
                                  {targetNoun(pp)}「{pp.target_name}」· {fieldLabel(pp)}
                                </div>
                                <div style={{ marginTop: 4, lineHeight: 1.7 }}>
                                  <div style={{ color: T.danger }}>− {cut((pp.before && pp.before.trim()) ? pp.before : before(pp)) || "（空）"}</div>
                                  <div style={{ color: T.ok }}>＋ {cut(pp.value) || "（空）"}</div>
                                </div>
                                </div>
                              </div>
                            ))}
                          </div>
                        </div>
                      )}

                      {/* 4b) 有问题但没有可执行补丁 → 明说原因 */}
                      {checkPatch.length === 0 && checkItems.length > 0 && (
                        <div style={{ marginTop: 10, padding: "8px 10px", borderRadius: 8, background: "rgba(255,122,24,.07)", border: "1px solid rgba(255,122,24,.25)", fontSize: 11, color: T.text2, lineHeight: 1.7 }}>
                          本次没有可自动替换的项 —— 上面这些问题需要你在对应页面自己改（例如<b>关系方向</b>这类字段不在自动替换范围内），或点「继续对话」补充说明后让 AI 再给一版建议。
                        </div>
                      )}

                      {/* 4c) 补丁里出现未知字段 → 提前警告（这批会被软件拒绝，不会撞数据库） */}
                      {checkPatch.some((pp) => fieldLabel(pp).startsWith("未知字段")) && (
                        <div style={{ marginTop: 10, padding: "8px 10px", borderRadius: 8, background: "rgba(240,85,77,.08)", border: "1px solid rgba(240,85,77,.3)", fontSize: 11, color: "#ff8c85", lineHeight: 1.7 }}>
                          ⚠ 有补丁使用了本板块不存在的字段（上面标着「未知字段」的那些）—— 这类条目**点了也不会执行**，
                          软件会在替换时报出原因。建议先点「继续对话」让 AI 重新给一版，或取消勾选它们。
                        </div>
                      )}

                      {/* 5) 「继续对话」输入 */}
                      <div style={{ marginTop: 12 }}>
                        <label style={{ fontSize: 11.5, color: T.text2 }}>继续对话（可选）—— 说明你的想法，让 AI 更新建议</label>
                        <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
                          <input value={checkInput} onChange={(e) => setCheckInput(e.target.value)}
                            onKeyDown={(e) => { if (e.key === "Enter") continueCheck(); }}
                            placeholder="例如：夏婉琴是女孩子，不要用“他”指代"
                            style={{ flex: 1, padding: "8px 11px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontSize: 12.5, outline: "none" }} />
                          <button onClick={continueCheck} disabled={checkBusy} style={{ height: 34, padding: "0 14px", borderRadius: 8, border: `1px solid ${T.line}`, background: T.panel2, color: T.text2, cursor: "pointer", fontSize: 12 }}>继续对话</button>
                        </div>
                      </div>
                    </>
                  )}

                  {checkNote && (
                    <div style={{ marginTop: 10, padding: "7px 10px", borderRadius: 8, background: "rgba(240,85,77,.08)", border: "1px solid rgba(240,85,77,.3)", fontSize: 11, color: "#ff8c85", lineHeight: 1.7 }}>
                      {checkNote}
                    </div>
                  )}

                  {/* 底部按钮 */}
                  {!noIssue && (
                    <div style={{ display: "flex", gap: 8, marginTop: 16, justifyContent: "flex-end", alignItems: "center" }}>
                      {checkBusy && <span style={{ fontSize: 11, color: T.text3, marginRight: "auto" }}>处理中…</span>}
                      <button onClick={() => setCheckBoard(null)} style={{ height: 32, padding: "0 14px", borderRadius: 8, border: `1px solid ${T.line}`, background: T.panel2, color: T.text2, cursor: "pointer", fontSize: 12 }}>取消</button>
                      <button onClick={applyCheckPatch} disabled={checkBusy || checkPick.size === 0}
                        title={checkPick.size === 0 ? "请先勾选要替换的条目" : "执行前会二次确认"}
                        style={{ height: 32, padding: "0 16px", borderRadius: 8, border: "none", background: checkPick.size === 0 ? T.panel3 : "linear-gradient(135deg,#FF7A18,#ff9a45)", color: checkPick.size === 0 ? T.text3 : "#231505", cursor: checkPick.size === 0 ? "not-allowed" : "pointer", fontSize: 12, fontWeight: 650 }}>
                        一键替换{checkPick.size > 0 ? `（${checkPick.size}）` : ""}
                      </button>
                    </div>
                  )}
                </div>
              </div>
            );
          })()}

          {/* C 子项：在"时间流逝"区间里插入事件 */}
          {gapPrompt && (
            <div style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,.55)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 60 }} onClick={() => setGapPrompt(null)}>
              <div onClick={(e) => e.stopPropagation()} style={{ width: 420, background: T.panel, border: `1px solid ${T.line}`, borderRadius: 14, padding: "18px 20px" }}>
                <div style={{ fontSize: 14, fontWeight: 650, marginBottom: 4 }}>⏳ 在这段里插入事件</div>
                <div style={{ fontSize: 11.5, color: T.text3, marginBottom: 12, lineHeight: 1.7 }}>
                  当前空白期：<b style={{ color: T.text2 }}>第 {gapPrompt.from} ~ {gapPrompt.to} 天</b>（这段时间没有剧情）<br />
                  插入后会在这段里新建一个天数节点；不插入也完全不影响现有内容。
                </div>
                {/* 规则说明 + 即时校验：早先只在提交后才报错，用户根本不知道有这两条限制（用户反馈）。
                    两条规则：① 只能填**区间内**的天；② **已经存在的那一天不能再建**（否则会出现两个「第 N 天」）。 */}
                {(() => {
                  const dn = Number(gapPrompt.day);
                  const inRange = dn >= gapPrompt.from && dn <= gapPrompt.to;
                  const exists = dayNodesSorted().some((x) => x.dayNo === dn);
                  const bad = !dn ? "请填写天数"
                    : (!Number.isInteger(dn) || dn <= 0) ? "天数必须是正整数"
                    : !inRange ? `只能填区间内的天数（第 ${gapPrompt.from} ~ ${gapPrompt.to} 天）`
                    : exists ? `第 ${dn} 天已经存在，不能重复创建`
                    : "";
                  return (
                    <div style={{ fontSize: 11, lineHeight: 1.8, padding: "7px 10px", borderRadius: 8, marginBottom: 10,
                      border: `1px solid ${bad ? T.danger + "55" : T.line}`,
                      background: bad ? "rgba(240,85,77,.08)" : T.panel2,
                      color: bad ? T.danger : T.text3 }}>
                      <div style={{ fontWeight: 600, color: bad ? T.danger : T.text2, marginBottom: 2 }}>
                        {bad ? "⚠ " + bad : "✅ 可以插入"}
                      </div>
                      规则：① 只能选<b>这段空白期内</b>的天数（第 {gapPrompt.from} ~ {gapPrompt.to} 天）；
                      ② <b>已经存在的天不能重复创建</b>（避免出现两个「第 N 天」）。
                    </div>
                  );
                })()}
                <label style={{ fontSize: 11.5, color: T.text2 }}>插入到第几天</label>
                <input value={gapPrompt.day} onChange={(e) => setGapPrompt({ ...gapPrompt, day: e.target.value.replace(/[^0-9]/g, "") })}
                  placeholder={`${gapPrompt.from} ~ ${gapPrompt.to}`}
                  style={{ width: "100%", boxSizing: "border-box", marginTop: 4, padding: "8px 11px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontFamily: "monospace", fontSize: 12.5, outline: "none" }} />
                <label style={{ fontSize: 11.5, color: T.text2, display: "block", marginTop: 10 }}>事件标题（可选，仅用于你自己备注）</label>
                <input value={gapPrompt.title} onChange={(e) => setGapPrompt({ ...gapPrompt, title: e.target.value })}
                  placeholder="例如：宸熙出差三天"
                  style={{ width: "100%", boxSizing: "border-box", marginTop: 4, padding: "8px 11px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontSize: 12.5, outline: "none" }} />
                <div style={{ display: "flex", gap: 8, marginTop: 14, justifyContent: "flex-end" }}>
                  <button onClick={() => setGapPrompt(null)} style={{ height: 30, padding: "0 14px", borderRadius: 8, border: `1px solid ${T.line}`, background: T.panel2, color: T.text2, cursor: "pointer", fontSize: 12 }}>取消</button>
                  {(() => {
                    const dn = Number(gapPrompt.day);
                    const ok = Number.isInteger(dn) && dn > 0
                      && dn >= gapPrompt.from && dn <= gapPrompt.to
                      && !dayNodesSorted().some((x) => x.dayNo === dn);
                    return (
                      <button onClick={insertEventAtDay} disabled={!ok}
                        title={ok ? "在这天插入一个事件节点" : "天数不合法或该天已存在，见上方提示"}
                        style={{ height: 30, padding: "0 14px", borderRadius: 8, border: "none",
                          background: ok ? "linear-gradient(135deg,#FF7A18,#ff9a45)" : T.line,
                          color: ok ? "#231505" : T.text3,
                          cursor: ok ? "pointer" : "not-allowed", fontSize: 12, fontWeight: 650 }}>
                        插入事件节点
                      </button>
                    );
                  })()}
                </div>
              </div>
            </div>
          )}

          {/* 底部输入台（仅剧情 view） */}
        {view === "story" && (
          <StoryInput
            activeCleanupRef={activeCleanupRef}
            aiRole={aiRole}
            autoAnalyze={autoAnalyze}
            autoSummarizeToday={autoSummarizeToday}
            backfillSummaries={backfillSummaries}
            chars={chars}
            constraints={constraints}
            ctxWindow={ctxWindow}
            currentStory={currentStory}
            dayDebug={dayDebug}
            directors={directors}
            genRequestId={genRequestId}
            genRequestRef={genRequestRef}
            input={input}
            lenLevel={lenLevel}
            loadChars={loadChars}
            loadDebug={loadDebug}
            loadEvals={loadEvals}
            loadProposals={loadProposals}
            loadTimelineAndMessages={loadTimelineAndMessages}
            memoryCtx={memoryCtx}
            msg={msg}
            myRole={myRole}
            navMode={navMode}
            node={node}
            pluginOn={pluginOn}
            recallDay={recallDay}
            recallDays={recallDays}
            sensitiveFilter={sensitiveFilter}
            setAiRole={setAiRole}
            setConstraints={setConstraints}
            setGenRequestId={setGenRequestId}
            setInput={setInput}
            setLenLevel={setLenLevel}
            setMemoryCtx={setMemoryCtx}
            setMessages={setMessages}
            setMsg={setMsg}
            setMyRole={setMyRole}
            setNavMode={setNavMode}
            setRecallDay={setRecallDay}
            setRecallDays={setRecallDays}
            setShowRecallPicker={setShowRecallPicker}
            updateDirectors={updateDirectors}
          />
        )}
        </div>

        {/* ③ 右侧信息面板（导演监视器 / 素材库） */}
        <div style={{ width: 300, borderLeft: `1px solid ${T.line}`, background: T.panel, flexShrink: 0, display: "flex", flexDirection: "column", minHeight: 0 }}>
          <div style={{ height: 52, flexShrink: 0, borderBottom: `1px solid ${T.line}`, display: "flex", alignItems: "center", gap: 8, padding: "0 16px" }}>
            <span style={{ color: T.accent, fontSize: 16 }}>🎬</span>
            <span style={{ fontSize: 13, fontWeight: 600 }}>导演监视器</span>
            <span style={{ marginLeft: "auto", color: T.text3, fontSize: 12, opacity: 0.45 }} title="面板设置（规划中）">⚙</span>
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
                      还没有角色
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
                    {/* 4 项属性条：数据本来就有（characters.stat_*），角色页也在用，这里直接展示真实数值 */}
                    <div style={{ padding: "11px 12px", borderTop: `1px solid ${T.lineSoft}`, display: "flex", flexDirection: "column", gap: 6 }}>
                      {([["体力", focusCh.stat_stamina, T.danger], ["理智", focusCh.stat_sanity, "#5b8cff"], ["情绪", focusCh.stat_emotion, "#f5b544"], ["戏份", focusCh.stat_screen, T.ok]] as [string, number, string][]).map(([lb, v, c]) => (
                        <div key={lb} style={{ display: "flex", alignItems: "center", gap: 8 }}>
                          <span style={{ fontSize: 10.5, color: T.text3, width: 28, flexShrink: 0 }}>{lb}</span>
                          <div style={{ flex: 1, height: 6, borderRadius: 3, background: "rgba(255,255,255,.07)", overflow: "hidden" }}>
                            <div style={{ width: `${Math.max(0, Math.min(100, v))}%`, height: "100%", background: c, borderRadius: 3 }} />
                          </div>
                          <span style={{ fontSize: 10, color: T.text3, width: 24, textAlign: "right", flexShrink: 0 }}>{v}</span>
                        </div>
                      ))}
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
            </div>
          </div>
        </div>

        {/* 设置弹窗 */}
        {showRecallPicker && (
          <div onClick={() => setShowRecallPicker(false)}
            style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,.55)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 60 }}>
            <div onClick={(e) => e.stopPropagation()}
              style={{ width: 560, maxHeight: "72vh", display: "flex", flexDirection: "column", border: `1px solid ${T.line}`, borderRadius: 14, background: T.panel, overflow: "hidden" }}>
              <div style={{ padding: "14px 16px", borderBottom: `1px solid ${T.line}`, display: "flex", alignItems: "center", gap: 9 }}>
                <span style={{ fontSize: 14, fontWeight: 650 }}>🔍 回顾哪一天</span>
                <span style={{ fontSize: 11.5, color: T.text3 }}>会把那天的原始对话拼进本次提示词</span>
                <button onClick={() => setShowRecallPicker(false)} style={{ marginLeft: "auto", background: "none", border: "none", color: T.text3, fontSize: 15, cursor: "pointer", fontFamily: "inherit" }}>✕</button>
              </div>
              <div style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: "10px 12px" }}>
                {recallDays.length === 0 ? (
                  <p style={{ fontSize: 12.5, color: T.text3 }}>
                    还没有可回顾的天 —— 需要**那天有聊过内容**（回顾注入的是原始对话，不依赖摘要）。
                  </p>
                ) : recallDays.map((d) => (
                  <button key={d.node_id}
                    onClick={() => { setRecallDay(d.node_id); setShowRecallPicker(false); setMsg(`🔍 本次会回顾「${d.date_label}」`); }}
                    style={{ display: "block", width: "100%", textAlign: "left", marginBottom: 6, padding: "9px 11px", borderRadius: 9, border: `1px solid ${recallDay === d.node_id ? T.accent : T.line}`, background: recallDay === d.node_id ? T.accentSoft : T.panel2, cursor: "pointer", fontFamily: "inherit" }}>
                    <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                      <span style={{ fontSize: 12.5, fontWeight: 600, color: recallDay === d.node_id ? T.accent : T.text1 }}>{d.date_label}</span>
                      <span style={{ fontSize: 10.5, color: T.text3 }}>{d.msg_count} 条对话</span>
                      {!d.has_summary && (
                        <span title="这天还没有日摘要 —— 手动回顾照样可用，只是自动检索找不到它"
                          style={{ fontSize: 9.5, padding: "1px 6px", borderRadius: 8, border: `1px dashed ${T.line}`, color: T.text3 }}>无摘要</span>
                      )}
                    </div>
                    <div style={{ fontSize: 11.5, color: T.text3, marginTop: 3, lineHeight: 1.6 }}>{d.preview}…</div>
                  </button>
                ))}
              </div>
              <div style={{ padding: "10px 16px", borderTop: `1px solid ${T.line}`, fontSize: 11, color: T.text3, lineHeight: 1.6 }}>
                回顾带进去的是**当天聊过的原始对话**（不是压缩后的摘要）—— 所以选「第 1 天」就能把第一天说过的原话拿回来。
                选了之后只在**下一次发送**生效。不选的话，软件会自己按你输入里的「第一次 / 那次 / 第 N 天」等词判断要不要回顾。
              </div>
            </div>
          </div>
        )}
        {showSettings && (
          <div style={{ position: "fixed", inset: 0, background: "rgba(8,9,12,.62)", backdropFilter: "blur(3px)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 100 }}>
            <div style={{ width: 560, maxHeight: "80vh", overflowY: "auto", background: T.panel, border: `1px solid ${T.line}`, borderRadius: 16, boxShadow: "0 24px 70px rgba(0,0,0,.6)" }}>
              <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "16px 20px", borderBottom: `1px solid ${T.line}` }}>
                <div style={{ width: 30, height: 30, borderRadius: 9, background: T.accentSoft, display: "flex", alignItems: "center", justifyContent: "center", color: T.accent, fontSize: 15 }}>⚙️</div>
                <div>
                  <h3 style={{ margin: 0, fontSize: 14.5, fontWeight: 650 }}>设置</h3>
                  <p style={{ margin: "2px 0 0", fontSize: 11.5, color: T.text3 }}>系统设置 · API / 临时文件</p>
                </div>
                <button onClick={() => setShowSettings(false)} style={{ marginLeft: "auto", color: T.text3, cursor: "pointer", padding: 4, borderRadius: 6, border: "none", background: "transparent", fontSize: 14 }}>✕</button>
              </div>
              <div style={{ padding: "18px 20px", display: "flex", flexDirection: "column", gap: 14 }}>
                <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
                  <button onClick={() => setSettingsTab("api")} style={{ padding: "7px 14px", borderRadius: 8, border: `1px solid ${T.line}`, cursor: "pointer", background: settingsTab === "api" ? T.accentSoft : "#191c22", color: settingsTab === "api" ? T.accent : T.text2, fontSize: 12.5, fontWeight: settingsTab === "api" ? 600 : 400 }}>🔑 API 设置</button>
                  <button onClick={() => { setShowSettings(false); setView("temp"); loadTemp(); }} style={{ padding: "7px 14px", borderRadius: 8, border: `1px solid ${T.line}`, cursor: "pointer", background: "#191c22", color: T.text2, fontSize: 12.5 }}>💾 数据管理</button>
                </div>
                {settingsTab === "api" && (
                  <div>
                    <div style={{ display: "grid", gridTemplateColumns: "110px 1fr", gap: 8, alignItems: "center" }}>
                      <label style={{ fontSize: 12, color: T.text2 }}>Base URL</label>
                      <input value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder="https://api.deepseek.com/v1" style={{ padding: "8px 11px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontFamily: "monospace", fontSize: 12, outline: "none" }} />
                      <label style={{ fontSize: 12, color: T.text2 }}>模型名</label>
                      <input value={modelName} onChange={(e) => setModelName(e.target.value)} placeholder="deepseek-chat" style={{ padding: "8px 11px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontFamily: "monospace", fontSize: 12, outline: "none" }} />
                      <label style={{ fontSize: 12, color: T.text2 }}>
                        API Key
                        <span style={{ marginLeft: 8, fontSize: 11, color: hasApiKey ? T.ok : T.warn }}>
                          {hasApiKey ? "● 已保存到系统凭据库" : "○ 尚未设置"}
                        </span>
                      </label>
                      <input type="password" value={apiKeyInput} onChange={(e) => setApiKeyInput(e.target.value)} placeholder="（留空则不修改已有 Key）" style={{ padding: "8px 11px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontFamily: "monospace", fontSize: 12, outline: "none" }} />
                      <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                        <button onClick={() => { void clearApiKey(); }} disabled={!hasApiKey} title="只删除系统凭据库里的这一条 Key" style={{ height: 28, padding: "0 10px", borderRadius: 7, border: `1px solid ${hasApiKey ? T.danger : T.line}`, background: "transparent", color: hasApiKey ? T.danger : T.text3, fontSize: 11.5, cursor: hasApiKey ? "pointer" : "not-allowed", fontFamily: "inherit" }}>🗑 清除已保存的 Key</button>
                      </div>
                      {/* 合规/安全说明：用户有权知道自己的密钥存在哪里 */}
                      <div style={{ fontSize: 10.5, color: T.text3 }}>密钥保存在 Windows 凭据管理器（不写入数据库、不进备份、不随作品导出）</div>
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
      {complianceModal}
      {snapPreviewModal}
      {branchDialogModal}
      {branchEditModal}
      </>
    );
}
export default App;
