import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { pluginCaps } from "./plugins";
import { LICENSE_NOTICE, NOTICE_VERSION, REQUIRED_NOTICES } from "./compliance";
import type { AppView,
  PortraitItem,
  SnapDiff,
  SnapItemChange,
  Story,
} from "./types";
import { loadPerformLexicon } from "./lib/chars";
import { useCheckPatch } from "./hooks/useCheckPatch";
import { useCompliance } from "./hooks/useCompliance";
import { usePlugins } from "./hooks/usePlugins";
import { useTimelineView } from "./hooks/useTimelineView";
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
import { PortraitPage } from "./views/PortraitPage";
import { CharsPage } from "./views/CharsPage";
import { WorldPage } from "./views/WorldPage";
import { ParamsPage } from "./views/ParamsPage";
import { DevToolsPage } from "./views/DevToolsPage";
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
  const { stories, setCurrentStory, currentStory, setShowNew, showNew, setNewTitle, newTitle, newOrigin, setNewOrigin, newPrologue, setNewPrologue, setDebugList, debugList, selectedDebug, setSelectedDebug, setChars, chars, charUsage, charsLoading, setRecallDay, recallDay, recallDays, setRecallDays, worlds, node, branches, currentBranchId, setCurrentBranchId, setBranchDialog, branchDialog, branchEdit, setBranchEdit, timelineNodes, setMessages, messages, relations, proposals, evals, setEvalBusy, evalBusy, setPluginState, pluginState, genList, genSel, setGenSel, setCloudForm, cloudForm, loadCloudConfig, loadGenerators, setRequireApproval, requireApproval, loadRequireApproval, loadProposals, loadEvals, snapshots, backupTakenRef, auditLogs, eventLogs, loadSnapshots, loadAudit, loadEventLogs, loadStories, loadDebug, backfillSummaries, autoSummarizeToday, loadBranches, switchBranch, createBranch, loadTimelineAndMessages, switchNode, loadChars, loadWorlds, loadRelations, ensureEditBackup } = session;
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
  const [view, setView] = useState<AppView>("home");
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
  // C 子项（时间流逝）：点区间条 → 在指定天数插入事件
  const [showExport, setShowExport] = useState(false);                        // 导出粒度弹窗
  const [pDetail, setPDetail] = useState<number | null>(null);
  const [portraits, setPortraits] = useState<PortraitItem[]>([]);
  const loadPortraits = async () => {
    try { setPortraits(await invoke<PortraitItem[]>("portrait_list")); } catch (e) { console.error(e); setMsg("❌ 加载素材失败: " + String(e)); }
  };
  // v3.5.1：表演标签词表（动作/情绪）只在后端维护一份，启动时取一次
  useEffect(() => { void loadPerformLexicon(); }, []);

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
  const navItems: { icon: string; label: string; view: "chars" | "world" | "proposals" | "timeline" | "stories" | "devtools" | null; action: (() => void) | null; sub?: boolean }[] = [
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
  // 生成结果一律烧录「AI」标识（显式角标 + 隐式元数据）：**固定行为，界面不提供开关**（用户 2026-10-12 定）。
  // 理由：AI 生成内容带标识是法规要求（《人工智能生成合成内容标识办法》2025-09-01 施行）；
  // 与其给一个"关掉标识"的按钮，不如让它成为软件的既定行为。实施位置见下面的 runGenerate。
  // 合规中心：「我已了解」确认（持久化）+ 自查清单勾选（本次会话内）
  // 第五批 ①：时间线树的折叠状态。
  // 现在**用不到**（线性故事一律全部展开，不显示折叠控件），但第 ③ 步分支系统会用到：
  // 分叉出现后，用户需要"只展开我正在看的那条线、收起别的分支"。
  // 保留它是为了到时候不必再改 treeRows 的判定逻辑 —— 那句 `!hasFork || treeExpanded.has(id)` 已经写好了。

  /**
   * **"当前分支能看到的节点"** = 共享历史（`branch_id IS NULL`）+ 当前分支自己的节点。
   *
   * 用户 2026-10-16 定：时间维/列表**只显示当前分支这一条线能看到的节点**，
   * 别的分支的节点**不出现**（原先"显示全树 + 打锁"的做法让用户以为数据错乱：
   * "切到主线还看得到别的分支的第 13~15 天，而且还改不了"）。
   * 想看全部分支的结构 → 切到 **🌲 时间线树状图**（那里永远显示全部）。
   */
  /**
   * **当前这条线"能看到"的节点 id 集合** = 共享历史（沿父链上溯到分叉点及之前）∪ 本分支自己的节点。
   *
   * 为什么要单独算：时间维只画"这条线"的内容，而**共享历史**与**别的分支**必须区别对待 ——
   * 前者要**逐个画出来**（第 1~11 天是共同经历），后者才收成一行"支线"。
   * 曾经把两者混为一谈 → 共享历史被整段跳成"时间流逝 · 第 2~11 天"（用户实测反馈）。
   *
   * 算法：从"本分支最后一个节点"（没有则分叉点）出发，沿 parent_id 上溯取全路径。
   */
  // 沿父链上溯的算法见 `lib/branchView.ts::branchPathIds`（踩过"共享历史被跳过"的坑，已单测）
  /** 当前分支能看到的节点（用于列表与时间维） */


  /** 可见节点 id 集合：折叠时子节点不显示（也要从时间线条里过滤掉） */

  // 合规的三段逻辑（读确认版本 / 关闭提示 / 确认）已抽到 `hooks/useCompliance.ts`
  const {
    // setter 只在 hook 内部用（对外只暴露状态 + 三个动作）
    complianceAck,
    complianceNotice,
    noticeAgreed, setNoticeAgreed,
    checklistDone, setChecklistDone,
    loadComplianceAck, dismissComplianceNotice, ackCompliance,
  } = useCompliance({ setMsg });

  // 时间线视图的状态与计算（列表/日历/树状图、空档、插入事件）—— 见 hooks/useTimelineView.ts
  // v3.5.5：剧情页头部（分支条/天数药丸）的 portal 宿主 —— 位于滚动容器外面，
  // 这样它**完全不参与滚动**（sticky 只是"贴顶"，仍会随容器移动）。
  const [storyHeaderEl, setStoryHeaderEl] = useState<HTMLDivElement | null>(null);


  const {
    timelineView, setTimelineView, calOffset, setCalOffset,
    gapPrompt, setGapPrompt,
    dayNodesSorted, dayNodes,
    insertEventAtDay, dayNoOf,
  } = useTimelineView({
    timelineNodes, currentBranchId, currentStory, setMsg, loadTimelineAndMessages,
  });

  // 插件运行期状态与动作（立绘生成通道这一块）：见 `hooks/usePlugins.ts`。
  // 以后的**内置插件**照这个模式各写一个 hook，App.tsx 只增加一行调用 + 渲染，
  // 不再往里堆 state。逻辑与拆分前逐行一致（含"生成必烧录 AI 标识"这条既定行为）。
  const {
    genUrl, setGenUrl, genProbing, setGenProbing, genProbe, setGenProbe,
    genBusy, genSummary, cloudSaving, setCloudSaving,
    genPrefill,
    openGenPanel, cloudOverrides, runGenerate, setPluginFlag, pluginKey, pluginOn,
  } = usePlugins({
    currentStory, setMsg, setView,
    pluginState, setPluginState, genList, genSel, cloudForm,
    loadGenerators, loadCloudConfig, loadPortraits,
  });

  /** 从角色卡发起：用角色资料拼基础提示词，并自动绑定该角色 */

  /** 从素材发起「重出这张」：带原提示词 / 负向 / 种子 / 尺寸，并挂到重出链上 */
  // 连接测试（只读探测，不产图）

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
      await invoke("create_story", {
        title: newTitle.trim(),
        timeOrigin: newOrigin,
        prologue: newPrologue,
      });
      setMsg(`✅ 已创建「${newTitle.trim()}」`);
      setShowNew(false);
      setNewTitle("");
      setNewPrologue("");
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

  /// v3.3：保存某故事的"前史/序章"。空串 = 清空（提示词里整段消失）。
  const saveStoryPrologue = async (id: number, prologue: string) => {
    try {
      await invoke("update_story_prologue", { id, prologue });
      await loadStories();
      setMsg(prologue.trim() ? "✅ 前史已保存" : "🗑 前史已清空");
    } catch (e) {
      setMsg("❌ 保存前史失败：" + String(e));
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
  // 「🔍 检查 → 一键替换」整套逻辑已抽到 `hooks/useCheckPatch.ts`：
  // 8 个 state + 字段映射 + 跑检查/继续对话/一键替换，逻辑与拆分前逐行一致。
  const {
    // 只取 App 真正用到的：弹窗那侧用的是 checkItems/checkPatch/checkNote 等，
    // 而 setCheckItems/setCheckPatch/checkPrev/字段映射都只在 hook 内部流转。
    checkBoard, setCheckBoard, checkBusy, checkItems,
    checkPatch, checkPick, setCheckPick,
    checkInput, setCheckInput, checkNote,
    runCheck, continueCheck, applyCheckPatch,
  } = useCheckPatch({ currentStory, chars, relations, worlds, setMsg, loadChars, loadWorlds, loadRelations });

  // 时间线相关的状态与计算已抽到 `hooks/useTimelineView.ts`：视图切换、日历偏移、
  // 时间流逝弹窗，以及 dayNodesSorted / timeGaps / nodeDateDisplay / dayNodes / 插入事件。
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
              <div style={{ fontSize: 12, fontWeight: 650, color: T.accent, marginBottom: 5 }}>📜 许可协议与免责声明</div>
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
                我已阅读并同意上述许可协议与免责声明；我理解 <b>AI 生成内容的标识与合规义务由我（使用者）承担</b>，
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
              <button onClick={() => { if (!currentStory) return; setShowExport(true); }} style={{ display: "inline-flex", alignItems: "center", gap: 5, height: 26, padding: "0 10px", borderRadius: 13, background: T.panel2, border: `1px solid ${T.line}`, fontSize: 11.5, color: T.text2, cursor: "pointer" }}>📥 导出</button>
              <button onClick={() => { setCurrentStory(null); setChars([]); setView("stories"); }} style={{ padding: "5px 12px", border: `1px solid ${T.line}`, borderRadius: 8, background: T.panel2, color: T.text2, cursor: "pointer", fontSize: 12 }}>← 返回故事库</button>
            </div>
          </div>

          {/* v3.5.5：**提示条**（用户 2026-10-24 要求）——
              放在"故事标题那一行"的**同一层级**、紧跟其下，属于滚动容器**外面**：
              所以它永远不会随内容滚动，也不会把剧情区的固定头部顶下去。
              它不并进上面那个盒子，而是新开一个 div 包着。 */}
          {msg && (
            <div
              style={{
                flexShrink: 0,
                padding: "6px 20px 8px",
                display: "flex",
                alignItems: "flex-start",
                gap: 8,
                borderBottom: `1px solid ${T.lineSoft}`,
                background: T.win,
              }}
              aria-live="polite"
            >
              <span
                style={{
                  fontSize: 11.5,
                  lineHeight: 1.6,
                  color: /^[❌⚠🛡]/.test(msg) ? T.warn : T.text2,
                  whiteSpace: "pre-wrap",
                  wordBreak: "break-word",
                }}
              >
                {msg}
              </span>
              <div style={{ flex: 1 }} />
              <button
                onClick={() => setMsg("")}
                title="关闭提示"
                style={{
                  flexShrink: 0, border: "none", background: "transparent", color: T.text3,
                  cursor: "pointer", fontSize: 12, lineHeight: 1, padding: "2px 4px", fontFamily: "inherit",
                }}
              >
                ✕
              </button>
            </div>
          )}

          {/* v3.5.5：剧情页头部的宿主节点 —— 与标题行同层级、在滚动容器外面 */}
          {view === "story" && <div ref={setStoryHeaderEl} style={{ flexShrink: 0, padding: "0 14px" }} />}

          <div ref={mainScrollRef} style={{ flex: 1, overflowY: "auto", padding: "20px 14px 12px" }}>

            {/* 故事列表（仅点击左侧"故事列表"时显示；首页为空白）
                宽度自适应：不再限宽，随中间工作区撑满（左右留白各 24px，来自上一层容器 padding） */}
        {view === "stories" && (
          <StoriesPage
            createStory={createStory}
            enterStory={enterStory}
            loadStories={loadStories}
            newOrigin={newOrigin}
            newPrologue={newPrologue}
            newTitle={newTitle}
            savePrologue={saveStoryPrologue}
            setNewPrologue={setNewPrologue}
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
            switchNode={switchNode}
            setCalOffset={setCalOffset}
            setMsg={setMsg}
            setTimelineView={setTimelineView}
            setView={setView}
            branches={branches}
            timelineView={timelineView}
            calOffset={calOffset}
            currentBranchId={currentBranchId}
            currentStory={currentStory}
            node={node}
            timelineNodes={timelineNodes}
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
        {import.meta.env.DEV && view === "devtools" && (
          <DevToolsPage />
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
            headerEl={storyHeaderEl}
            branches={branches}
            cancelEdit={cancelEdit}
            chars={chars}
            currentBranchId={currentBranchId}
            currentStory={currentStory}
            editText={editText}
            editingId={editingId}
            loadBranches={loadBranches}
            loadChars={loadChars}
            loadRelations={loadRelations}
            loadTimelineAndMessages={loadTimelineAndMessages}
            loadWorlds={loadWorlds}
            messages={messages}
            node={node}
            setBranchDialog={setBranchDialog}
            setBranchEdit={setBranchEdit}
            setCurrentBranchId={setCurrentBranchId}
            setEditText={setEditText}
            setEditingId={setEditingId}
            setMsg={setMsg}
            switchBranch={switchBranch}
            timelineNodes={timelineNodes}
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
