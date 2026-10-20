import { useEffect, useRef, useState, type ReactElement } from "react";
import { invoke } from "@tauri-apps/api/core";
import { pluginCaps, pluginConfigKey } from "./plugins";
import type { GenProbe } from "./plugins";
import { LICENSE_NOTICE, NOTICE_VERSION, REQUIRED_NOTICES } from "./compliance";
import type {
  PortraitItem,
  SnapDiff,
  SnapItemChange,
  Story,
  TimelineNodeDto,
  CheckItemT,
  PatchItemT,
} from "./types";
import { nowStamp } from "./lib/time";
import {

  PLUGIN_REGISTRY,

} from "./constants";
import { T } from "./theme";
import {


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
import { DirectorPanel } from "./views/DirectorPanel";
import { ExportModal, type ExportOptions } from "./views/ExportModal";
import { SettingsModal } from "./views/SettingsModal";
import { CheckReportModal } from "./views/CheckReportModal";
import { InsertEventModal } from "./views/InsertEventModal";
import { useStorySession } from "./hooks/useStorySession";

function App() {
  const [msg, setMsg] = useState("");
  const [focusCharId, setFocusCharId] = useState<number | null>(null);
  const session = useStorySession({ setFocusCharId, setMsg });
  const { stories, setCurrentStory, currentStory, setShowNew, showNew, setNewTitle, newTitle, newOrigin, setNewOrigin, setDebugList, debugList, selectedDebug, setSelectedDebug, setChars, chars, charUsage, charsLoading, setRecallDay, recallDay, recallDays, setRecallDays, worlds, node, branches, currentBranchId, setCurrentBranchId, setBranchDialog, branchDialog, branchEdit, setBranchEdit, timelineNodes, setMessages, messages, relations, proposals, evals, setEvalBusy, evalBusy, setPluginState, pluginState, genList, genSel, setGenSel, setCloudForm, cloudForm, loadCloudConfig, loadGenerators, setRequireApproval, requireApproval, loadRequireApproval, loadProposals, loadEvals, snapshots, backupTakenRef, auditLogs, eventLogs, loadSnapshots, loadAudit, loadEventLogs, loadStories, loadDebug, backfillSummaries, autoSummarizeToday, loadBranches, switchBranch, createBranch, loadTimelineAndMessages, switchNode, removeNode, loadChars, loadWorlds, loadRelations, ensureEditBackup } = session;
  // ===== 故事库（M1）=====
  /// 中间工作区滚动容器：**切页时回到顶部**。
  /// 不重置的话，从长页面（如「待确认设定」）切过去时，
  /// 页面顶部的提示信息（全局 msg）会停在屏幕外，表现成"点了没反应"。
  const mainScrollRef = useRef<HTMLDivElement | null>(null);
  // 对话区自动跟随：AI 逐字回复时把视图底部贴住最新内容；用户一滚轮就停（见下面的 effect）
  const followBottomRef = useRef(false);

  // ===== 设置（M0/M2）=====
  const [showSettings, setShowSettings] = useState(false);
  const [modelName, setModelName] = useState("");
  // 主模型 Key 是否已在系统凭据库中（界面只显示"已保存"，不显示明文）

  // ===== 请求记录（调试用；只读，不可回滚）=====

  // ===== 角色库（工作区内）=====
  // ===== 角色管理页（A 卡墙 / D 关系图谱 / G 新建表单）=====
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
  /// 「🔍 回顾」弹窗是否打开
  const [showRecallPicker, setShowRecallPicker] = useState(false);
  /// 可选的天（本分支可见 + 有日摘要）
  // 导演指令（沉浸对话专用，一次性：附加到下一次发送后自动清空）
  const [directors, setDirectors] = useState<string[]>(() => { try { return JSON.parse(localStorage.getItem("ai.directors") || "[]"); } catch { return []; } });
  const [timelineView, setTimelineView] = useState<"list" | "cal" | "tree">("list");   // 时间线视图：列表/日历/树状图
  const [calOffset, setCalOffset] = useState(0);                               // 日历翻页偏移（月）
  // C 子项（时间流逝）：点区间条 → 在指定天数插入事件
  const [gapPrompt, setGapPrompt] = useState<{ from: number; to: number; day: string; title: string } | null>(null);
  const [showExport, setShowExport] = useState(false);                        // 导出粒度弹窗
  const [pDetail, setPDetail] = useState<number | null>(null);
  const [portraits, setPortraits] = useState<PortraitItem[]>([]);
  const loadPortraits = async () => {
    try { setPortraits(await invoke<PortraitItem[]>("portrait_list")); } catch (e) { console.error(e); setMsg("❌ 加载素材失败: " + String(e)); }
  };
  useEffect(() => { loadPortraits(); }, []);
  // ===== 生成参数页（左侧导航：立绘素材下方）=====
  const [ctxWindow, setCtxWindow] = useState(32768);
  // 上下文预算（A 子项）：设置页三个输入 + 本地即时校验
  const [nRecentInput, setNRecentInput] = useState("10");   // 每日上限：N 的封顶值
  const [autoAnalyze, setAutoAnalyze] = useState(true);      // 第三批：回复后自动分析演进
  const [pParamId, setPParamId] = useState<number | null>(null);
  const [pDebugId, setPDebugId] = useState<number | null>(null);
  // 「未开放的生成参数」折叠区（原来是 5 个散落的虚框，很杂）
  const [pParamForm, setPParamForm] = useState({ prompt: "", negative: "", seed: "", steps: "", cfg: "", model: "" });
  // sub = 故事列表下的二级入口（展开后缩进显示）
  const navItems: { icon: string; label: string; view: "chars" | "world" | "proposals" | "timeline" | "stories" | null; action: (() => void) | null; sub?: boolean }[] = [
    { icon: "📁", label: "故事列表", view: "stories", action: () => { setCurrentStory(null); setChars([]); setView("stories"); } },
    { icon: "📅", label: "时间线", view: "timeline", action: null, sub: true },
    { icon: "👥", label: "角色管理", view: "chars", action: null, sub: true },
    { icon: "🌍", label: "世界观设定", view: "world", action: null, sub: true },
    { icon: "📋", label: "演进提案", view: "proposals", action: null, sub: true },
  ];

  // ===== 剧情（工作区内）=====

  // 第五批 ③：分支状态。`currentBranchId = null` 表示主线。
  //
  // ⚠ **必须声明在"用到它的派生值"之前**：下面 `visibleForBranch` / `branchScopedNodes`
  // 会在**每次渲染时立刻求值**，若 state 声明在后面就撞上 TDZ
  // （实测白屏：`ReferenceError: Cannot access 'currentBranchId' before initialization`）。
  // 这类错误只在运行时报，`tsc` 检查不出来。
  // 第五批「主生成链路」修复（《现状说明》5.2）：记录本次生成的 requestId，
  // "停止"按钮带着它调 cancel_generation。null = 当前没有在生成。
  const [genRequestId, setGenRequestId] = useState<string | null>(null);
  const genRequestRef = useRef<string | null>(null);   // 给事件回调读（避免闭包拿到旧 state）
  // 第五批（《现状说明》5.3）：**卸载时要跑的清理**（注销流式监听器）
  const activeCleanupRef = useRef<(() => void) | null>(null);

  // 第五批 ④：分支改名 / 备注（管理平行故事）。null = 弹窗关闭。
  const [input, setInput] = useState("");
  const [editingId, setEditingId] = useState<number | null>(null);
  const [editText, setEditText] = useState("");

  // ===== 关系（工作区内）=====
  // 关系图谱父节点（null = 自动取焦点角色/第一个角色）
  const [graphRootId, setGraphRootId] = useState<number | null>(null);
  // 当前角色卡焦点角色（null = 默认第一个有状态的角色；点击右侧立绘区才切换）

  // ===== 设定演进提案（M6）=====
  // 插件运行期状态（注册表驱动：加插件只需在 PLUGIN_REGISTRY 里加一条）
  const setPluginFlag = (id: string, patch: Partial<{ enabled: boolean; panelOpen: boolean }>) =>
    setPluginState((prev) => ({ ...prev, [id]: { ...(prev[id] ?? { enabled: false, panelOpen: false }), ...patch } }));
  // 第四批 ⑥ P1：立绘生成通道（后端注册表下发，如实报告每个通道是否已实装）
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
      await invoke("set_config", { key: "compliance.notice_seen", value: nowStamp() });
      setComplianceAck(true);
      setMsg("✅ 已记录：你已阅读并同意当前条款（版本 " + NOTICE_VERSION + "）");
    } catch { /* 忽略 */ }
  };
  const ackCompliance = async () => {
    try {
      await invoke("set_config", { key: "compliance.acknowledged_at", value: nowStamp() });
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
  const [cloudSaving, setCloudSaving] = useState(false);

  // 读已保存的云端配置（Key 只回传"是否已设置"）


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
  /// 批量勾选：选中的提案组 id
  const [selGroups, setSelGroups] = useState<Set<number>>(new Set());
  /// **就地**显示"上次分析的结果"。
  ///
  /// 为什么必须有它：全局 `msg` 渲染在滚动区**最顶部**，而本页很长、
  /// 切页时滚动位置不重置 —— 用户点「分析」后提示在屏幕外，
  /// 表现就是"点了没反应"（用户实测反馈）。
  const [analyzeInfo, setAnalyzeInfo] = useState<{ text: string; bad: boolean } | null>(null);
  /// 是否展开"已处理"历史



  // 载入评估记录（不需要 API）

  // 手动评估当前节点最后一条 AI 回复（会调 API，有成本）


  /// 注入一条测试提案（不依赖 AI 剧情信号）

  /// ⑤-B：批量落地选中的组（后端整批只打一个快照）



  // ===== M7：设定版本 / 审计 / 事件日志 / 缓存 =====
  // 第五批 ②：回滚前的差异预览（null = 未打开）。回滚是破坏性操作，必须先看清会失去什么。
  const [snapPreview, setSnapPreview] = useState<{
    version: number; diff: SnapDiff; loading: boolean; error: string;
  } | null>(null);
  // 第五批 ②（B）：本次会话是否已经为"直接编辑"存过临时备份（同一会话只存第一次）

  // 「临时文件」页：temp/ 里的 AI 出图中间文件（入库失败/取消会残留）
  type TempFile = { name: string; size: number; age_secs: number };
  const [tempFiles, setTempFiles] = useState<TempFile[]>([]);

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




  useEffect(() => {
    loadStories();
    loadSettings();
    // 启动时就要判断"首次使用"——否则弹窗要等用户点进「合规与责任」才出现（实测踩过这个坑）
    void loadComplianceAck();
  }, []);

  const loadSettings = async () => {
    try {
      // App 只读"已生效值"（顶栏模型名 / 参数页 N / 生成流程的 autoAnalyze）；
      // 其余草稿与 Key 状态由「设置」弹窗自己读取（见 views/SettingsModal.tsx）。
      const mn = await invoke<string | null>("get_config", { key: "model.name" });
      setModelName(mn ?? "");
      const cw = await invoke<string | null>("get_config", { key: "model.context_window" });
      if (cw) { const n = Number(cw); if (n > 0) setCtxWindow(n); }
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


  /// 打开「🔍 回顾」弹窗时拉取可选的"天"




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
  // 当天到量自动摘要（阈值：AI 回复条数）

  // ===== 第五批 ③：分支 =====
  /** 读该故事的分支列表与"当前分支"（进入故事 / 切换后调用） */

  /** 切换分支：设定后端当前分支 → 重新加载全部相关数据（时间线/消息/角色/关系/世界观/版本/提案） */

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



  const updateDirectors = (list: string[]) => { setDirectors(list); localStorage.setItem("ai.directors", JSON.stringify(list)); };

  const doExport = async (o: ExportOptions) => {
    if (!currentStory) return;
    let ids: number[] = [];
    let label = "";
    if (o.scope === "current") {
      if (!node) { setMsg("⚠ 当前没有选中的节点，请先在时间线或剧情页选择节点"); return; }
      ids = [node.id];
      label = "当前节点";
    } else if (o.scope === "range") {
      const lo = Math.min(o.from, o.to), hi = Math.max(o.from, o.to);
      ids = dayNodes.filter((n) => { const d = dayNoOf(n); return d >= lo && d <= hi; }).map((n) => n.id);
      label = `第 ${lo} 天 ~ 第 ${hi} 天（不含插曲）`;
      if (ids.length === 0) { setMsg("⚠ 该区间内没有节点"); return; }
    } else {
      label = "全部节点";
    }
    const modeLabel = o.mode === "narr" ? "仅旁白" : o.mode === "chat" ? "仅对话" : "全部";
    const cmd = o.fmt === "md" ? "export_story_md" : o.fmt === "html" ? "export_story_html" : "export_story_txt";
    const fmtLabel = o.fmt === "md" ? "MD" : o.fmt === "html" ? "HTML" : "TXT";
    setMsg(`⏳ 正在导出 ${fmtLabel}…`);
    try {
      const r = await invoke(cmd, { storyId: String(currentStory.id), nodeIds: ids, modeFilter: o.mode, scopeLabel: `${label} · 模式 ${modeLabel}` });
      setMsg(`✅ 已导出 ${fmtLabel}：` + String(r));
      setShowExport(false);
    } catch (e) { setMsg("❌ 导出失败：" + String(e)); }
  };




  // 第五批（《现状说明》5.3）：组件卸载时注销流式监听器（防泄漏）
  useEffect(() => () => {
    try { activeCleanupRef.current?.(); } catch { /* 忽略 */ }
    activeCleanupRef.current = null;
  }, []);


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
  // CheckItemT / PatchItemT 已搬到 types.ts（C2：弹窗组件也要用同一份定义）
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

  // ===== 对话区自动跟随（2026-10-18 用户要求）=====
  //
  // 行为：点「发送消息」后，AI 回复逐字变长时，视图**底部跟随**最新内容；
  // 用户一旦滚动（滚轮 / 触摸 / 拖滚动条），立刻停止跟随 —— 不打搅回看历史。
  //
  // 为什么用 wheel / touchstart / mousedown 判断"用户滚"：
  // 程序自己改 scrollTop **不会**触发这些事件，所以能干净地区分"用户在滚"与"我们在滚"。
  const scrollToBottom = () => {
    const el = mainScrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  };

  // 开始一次生成 → 重新打开跟随 + 先贴到底（让新气泡可见）
  useEffect(() => {
    if (view !== "story") return;
    if (genRequestId) {
      followBottomRef.current = true;
      scrollToBottom();
    }
  }, [genRequestId, view]);

  // 消息变化（流式增量 / 新气泡）→ 跟随到底
  useEffect(() => {
    if (view !== "story") return;
    if (followBottomRef.current) scrollToBottom();
  }, [messages, view]);

  // 用户滚动 → 停止跟随（只挂一次）
  useEffect(() => {
    const el = mainScrollRef.current;
    if (!el) return;
    const stop = () => { followBottomRef.current = false; };
    el.addEventListener("wheel", stop, { passive: true });
    el.addEventListener("touchstart", stop, { passive: true });
    el.addEventListener("mousedown", stop);
    return () => {
      el.removeEventListener("wheel", stop);
      el.removeEventListener("touchstart", stop);
      el.removeEventListener("mousedown", stop);
    };
  }, []);

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
            setMsg={setMsg}
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

          {/* 第二批 · ② 检查：报告弹窗（C2 抽到 views/CheckReportModal.tsx） */}
          {checkBoard && (
            <CheckReportModal
              applyCheckPatch={applyCheckPatch}
              chars={chars}
              relations={relations}
              worlds={worlds}
              checkBoard={checkBoard}
              checkBusy={checkBusy}
              checkInput={checkInput}
              checkItems={checkItems}
              checkNote={checkNote}
              checkPatch={checkPatch}
              checkPick={checkPick}
              continueCheck={continueCheck}
              setCheckBoard={setCheckBoard}
              setCheckInput={setCheckInput}
              setCheckPick={setCheckPick}
            />
          )}

          {/* C 子项：在"时间流逝"区间里插入事件（C2 抽到 views/InsertEventModal.tsx） */}
          {gapPrompt && (
            <InsertEventModal
              dayNodesSorted={dayNodesSorted}
              gapPrompt={gapPrompt}
              insertEventAtDay={insertEventAtDay}
              setGapPrompt={setGapPrompt}
            />
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

        {/* ③ 右侧信息面板（导演监视器 / 素材库）—— 2026-10-20 抽到 views/DirectorPanel.tsx */}
        <DirectorPanel
          chars={chars}
          portraits={portraits}
          relations={relations}
          currentStory={currentStory}
          focusCharId={focusCharId}
          setFocusCharId={setFocusCharId}
          graphRootId={graphRootId}
          setGraphRootId={setGraphRootId}
          setView={setView}
          setPDetail={setPDetail}
          ctxWindow={ctxWindow}
          timelineNodes={timelineNodes}
          node={node}
        />

        {/* 🔍 回顾哪一天（把某天的原始对话拼进本次提示词；选完在下一次发送生效） */}
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
                    {/* 检索标签：手打标签 + 从日摘要派生的（A+C）。最多显示 6 个，其余折叠成 +N */}
                    {d.tags && d.tags.length > 0 && (
                      <div style={{ display: "flex", gap: 4, flexWrap: "wrap", marginTop: 5 }}>
                        {d.tags.slice(0, 6).map((tg) => (
                          <span key={tg} title="这一天的检索标签：手打标签 + 从日摘要派生"
                            style={{ fontSize: 9.5, padding: "1px 6px", borderRadius: 8,
                              border: `1px solid ${T.accent}44`, background: `${T.accent}14`, color: T.accent }}>
                            #{tg}
                          </span>
                        ))}
                        {d.tags.length > 6 && (
                          <span style={{ fontSize: 9.5, padding: "1px 5px", color: T.text3 }}>+{d.tags.length - 6}</span>
                        )}
                      </div>
                    )}
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
        {/* ⚙️ 设置弹窗（2026-10-20 抽到 views/SettingsModal.tsx） */}
        {showSettings && (
          <SettingsModal
            initial={{ modelName, nRecent: nRecentInput, autoAnalyze }}
            onClose={() => setShowSettings(false)}
            onOpenData={() => { setShowSettings(false); setView("temp"); loadTemp(); }}
            onApplied={(v) => {
              setModelName(v.modelName);
              setNRecentInput(v.nRecent);
              setAutoAnalyze(v.autoAnalyze);
              if (v.ctxWindow > 0) setCtxWindow(v.ctxWindow);
            }}
            onMsg={setMsg}
          />
        )}

        {showExport && (
          <ExportModal
            days={dayNodes}
            dayNoOf={dayNoOf}
            onClose={() => setShowExport(false)}
            onExport={doExport}
          />
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
