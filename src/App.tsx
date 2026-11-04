import { useCallback, useEffect, useRef, useState, type ComponentProps, type Dispatch, type SetStateAction } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AppView, SnapDiff, Story } from "./types";
import { loadPerformLexicon } from "./lib/chars";
import { useCheckPatch } from "./hooks/useCheckPatch";
import { useCompliance } from "./hooks/useCompliance";
import { usePlugins } from "./hooks/usePlugins";
import { useTimelineView } from "./hooks/useTimelineView";
import { useTempFiles } from "./hooks/useTempFiles";
import { useAppSettings } from "./hooks/useAppSettings";
import { usePortraits } from "./hooks/usePortraits";
import { useStoryInputPrefs } from "./hooks/useStoryInputPrefs";
import { T } from "./theme";
import { TempPage } from "./views/TempPage";
import { CompliancePage } from "./views/CompliancePage";
import { TimelinePage } from "./views/TimelinePage";
import { ChaptersPage } from "./views/ChaptersPage";
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
import { SnapshotPreviewModal } from "./views/SnapshotPreviewModal";
import { ComplianceModal } from "./views/ComplianceModal";
import { BranchEditDialog, CreateBranchDialog } from "./views/BranchDialogs";
import { useStorySession } from "./hooks/useStorySession";
import { useFollowBottom } from "./hooks/useFollowBottom";
import { useViewEffects } from "./hooks/useViewEffects";
import { useConfirm } from "./confirm";
import { useToast } from "./toast";

function App() {
  const confirm = useConfirm();
  const toast = useToast();
  const [msg, setMsg] = useState("");
  const [focusCharId, setFocusCharId] = useState<number | null>(null);

  // ===== 提示分层（见 toast.tsx 顶部的 T1/T2/T3 约定）=====
  // `msg` 只给剧情区用（StoryInput 里那个浮层）；**其它页面的操作结果一律走 toast**，
  // 否则会出现"在插件页点保存、提示却飘在对话区"的情况（2026-10-25 用户反馈）。
  // 这个包装函数的签名与 setMsg 一致，所以可以直接透传给各页面的 setMsg 属性。
  const toastMsg: Dispatch<SetStateAction<string>> = useCallback((v) => {
    const s = typeof v === "function" ? v("") : v;
    if (s && s.trim()) toast(s);
  }, [toast]);

  const session = useStorySession({ setFocusCharId, setMsg });
  const { stories, setCurrentStory, currentStory, setShowNew, showNew, setNewTitle, newTitle, newOrigin, setNewOrigin, newPrologue, setNewPrologue, setDebugList, debugList, selectedDebug, setSelectedDebug, setChars, chars, charUsage, charsLoading, setRecallPick, recallPick, recallDays, setRecallDays, worlds, node, branches, currentBranchId, setCurrentBranchId, setBranchDialog, branchDialog, branchEdit, setBranchEdit, timelineNodes, setMessages, messages, relations, proposals, setPluginState, pluginState, genList, genSel, setGenSel, setCloudForm, cloudForm, loadCloudConfig, loadGenerators, setRequireApproval, requireApproval, loadRequireApproval, loadProposals, pardons, loadPardons, snapshots, backupTakenRef, auditLogs, eventLogs, loadSnapshots, loadAudit, loadEventLogs, loadStories, loadDebug, backfillSummaries, autoSummarizeToday, loadBranches, switchBranch, createBranch, loadTimelineAndMessages, switchNode, loadChars, loadWorlds, loadRelations, ensureEditBackup , removeNode} = session;
  // ===== 故事库（M1）=====
  /// 中间工作区滚动容器：**切页时回到顶部**。
  /// 不重置的话，从长页面（如「待确认设定」）切过去时，
  /// 页面顶部的提示信息（全局 msg）会停在屏幕外，表现成"点了没反应"。
  const mainScrollRef = useRef<HTMLDivElement | null>(null);

  // ===== 设置（M0/M2）=====
  // 主模型 Key 是否已在系统凭据库中（界面只显示"已保存"，不显示明文）

  // ===== 请求记录（调试用；只读，不可回滚）=====

  // ===== 角色库（工作区内）=====
  // ===== 角色管理页（A 卡墙 / D 关系图谱 / G 新建表单）=====
  const [charGraphRootId, setCharGraphRootId] = useState<number | null>(null);

  // ===== 世界观（工作区内）=====
  const [view, setView] = useState<AppView>("home");
  // 敏感词过滤：**内置常开，不提供开关**（拦截输入 + 过滤 AI 输出）。
  // 保留常量名以便下游判断逻辑不变；不再有 setState —— 界面上的按钮改为状态指示。
  const sensitiveFilter = true;

  // 剧情输入台的偏好（旁白/对话、我演谁、约束、篇幅、记忆、导演指令、回顾、导出弹窗）
  // —— 见 hooks/useStoryInputPrefs.ts（其中约束与导演指令持久化在 localStorage）
  const {
    navMode, setNavMode, myRole, setMyRole, aiRole, setAiRole,
    constraints, updateConstraints, lenLevel, setLenLevel,
    memoryCtx, setMemoryCtx, showRecallPicker, setShowRecallPicker,
    directors, updateDirectors, showExport, setShowExport,
  } = useStoryInputPrefs();
  // ===== RAG ①：回顾 =====
  /// 界面「🔍 回顾」指定的某一天（node_id）。有值时后端**强制**把那天拼进提示词。
  /// 「🔍 回顾」弹窗是否打开
  /// 可选的天（本分支可见 + 有日摘要）
  // 导演指令（沉浸对话专用，一次性：附加到下一次发送后自动清空）
  // C 子项（时间流逝）：点区间条 → 在指定天数插入事件
  // v3.5.1：表演标签词表（动作/情绪）只在后端维护一份，启动时取一次
  useEffect(() => { void loadPerformLexicon(); }, []);

  useEffect(() => { loadPortraits(); }, []);
  // ===== 生成参数页（左侧导航：立绘素材下方）=====
  // 上下文预算（A 子项）：设置页三个输入 + 本地即时校验
  // 「未开放的生成参数」折叠区（原来是 5 个散落的虚框，很杂）
  // sub = 故事列表下的二级入口（展开后缩进显示）
  const navItems: { icon: string; label: string; view: "chars" | "world" | "proposals" | "timeline" | "chapters" | "stories" | "devtools" | null; action: (() => void) | null; sub?: boolean }[] = [
    { icon: "📁", label: "故事列表", view: "stories", action: () => { setCurrentStory(null); setChars([]); setView("stories"); } },
    { icon: "📅", label: "时间线", view: "timeline", action: null, sub: true },
    { icon: "👥", label: "角色管理", view: "chars", action: null, sub: true },
    { icon: "🌍", label: "世界观设定", view: "world", action: null, sub: true },
    { icon: "📖", label: "历史章节", view: "chapters", action: null, sub: true },
    { icon: "📋", label: "演进提案", view: "proposals", action: null, sub: true },
  ];

  // ===== 剧情（工作区内）=====

  // 第五批 ③：分支状态。`currentBranchId = null` 表示主线。
  //
  // ⚠ 分支相关 state **必须声明在"用它算派生值"的代码之前**：
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
  } = useCompliance({ setMsg: toastMsg });

  // 时间线视图的状态与计算（列表/日历/树状图、空档、插入事件）—— 见 hooks/useTimelineView.ts
  // v3.5.5：剧情页头部（分支条/天数药丸）的 portal 宿主 —— 位于滚动容器外面，
  // 这样它**完全不参与滚动**（sticky 只是"贴顶"，仍会随容器移动）。
  const [storyHeaderEl, setStoryHeaderEl] = useState<HTMLDivElement | null>(null);
  /**
   * 稳定的 ref 回调。
   *
   * 为什么不能直接 `ref={setStoryHeaderEl}`：内联/每次新建的回调**身份每次渲染都变**，
   * React 会按"卸载旧 ref + 挂载新 ref"处理 → 每帧 `setState(null)` 再 `setState(el)`，
   * 状态一抖就可能自己触发自己。这里用 `useCallback(..., [])` 固定身份，
   * 并且用函数式 setState 在值相同时**不触发更新**。
   */
  const attachStoryHeader = useCallback((el: HTMLDivElement | null) => {
    setStoryHeaderEl((prev) => (prev === el ? prev : el));
  }, []);


  const {
    timelineView, setTimelineView, calOffset, setCalOffset,
    gapPrompt, setGapPrompt, timeGaps,
    dayNodesSorted, dayNodes,
    insertEventAtDay, dayNoOf,
  } = useTimelineView({
    timelineNodes, currentBranchId, currentStory, setMsg, loadTimelineAndMessages,
  });

  // 临时文件（数据管理页）—— 见 hooks/useTempFiles.ts
  const { tempFiles, loadTemp, cleanTemp } = useTempFiles({ setMsg: toastMsg });

  // 插件运行期状态与动作（立绘生成通道这一块）：见 `hooks/usePlugins.ts`。
  // 以后的**内置插件**照这个模式各写一个 hook，App.tsx 只增加一行调用 + 渲染，
  // 不再往里堆 state。逻辑与拆分前逐行一致（含"生成必烧录 AI 标识"这条既定行为）。
  // 立绘素材（素材库 / 参数页 / 角色卡 / 导演监视器共用）—— 见 hooks/usePortraits.ts
  // ⚠ 必须声明在 usePlugins **之前**：它的 deps 里要用 loadPortraits
  const {
    portraits, loadPortraits,
    pDetail, setPDetail,
    pDebugId, setPDebugId,
  } = usePortraits({ setMsg: toastMsg });

  const {
    genUrl, setGenUrl, genProbing, setGenProbing, genProbe, setGenProbe,
    genBusy, genSummary, cloudSaving, setCloudSaving,
    genPrefill,
    openGenPanel, cloudOverrides, runGenerate, setPluginFlag, pluginKey,
  } = usePlugins({
    // 插件的提示（立绘生成失败 / 保存云端配置 / 评估完成）都属于**操作结果** → toast。
    // 这里曾经传的是剧情区的 setMsg，于是"绘图失败"的提示会飘在对话区（2026-10-25 用户实测）。
    currentStory, setMsg: toastMsg, setView,
    pluginState, setPluginState, genList, genSel, cloudForm, loadCloudConfig, loadPortraits, loadGenerators,
  });

  // 设置/参数页的状态（模型名、上下文窗口、N、自动分析）—— 见 hooks/useAppSettings.ts
  const {
    showSettings, setShowSettings, modelName, setModelName,
    ctxWindow, setCtxWindow, nRecentInput, setNRecentInput,
    autoAnalyze, setAutoAnalyze, loadSettings,
  } = useAppSettings({
    setPluginState,
    getPluginState: () => pluginState,
    pluginKey,
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



  /// 打开「🔍 回顾」弹窗时拉取可选的"天"




  const createStory = async () => {
    if (!newTitle.trim()) {
      toastMsg("❌ 标题不能为空");
      return;
    }
    try {
      await invoke("create_story", {
        title: newTitle.trim(),
        timeOrigin: newOrigin,
        prologue: newPrologue,
      });
      toastMsg(`✅ 已创建「${newTitle.trim()}」`);
      setShowNew(false);
      setNewTitle("");
      setNewPrologue("");
      await loadStories();
    } catch (e) {
      toastMsg("❌ 创建失败: " + String(e));
    }
  };

  const removeStory = async (id: number, title: string) => {
    if (!(await confirm({
      title: "删除故事",
      body: `确定删除故事「${title}」？\n\n它的 story.db（全部剧情、消息、立绘记录）会一起删掉，不可撤销。`,
      danger: true,
    }))) return;
    try {
      await invoke("delete_story", { id });
      toastMsg(`🗑️ 已删除「${title}」`);
      await loadStories();
    } catch (e) {
      toastMsg("❌ 删除失败: " + String(e));
    }
  };

  /// v3.3：保存某故事的"前史/序章"。空串 = 清空（提示词里整段消失）。
  const saveStoryPrologue = async (id: number, prologue: string) => {
    try {
      await invoke("update_story_prologue", { id, prologue });
      await loadStories();
      toastMsg(prologue.trim() ? "✅ 前史已保存" : "🗑 前史已清空");
    } catch (e) {
      toastMsg("❌ 保存前史失败：" + String(e));
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
  } = useCheckPatch({ currentStory, chars, relations, worlds, setMsg: toastMsg, loadChars, loadWorlds, loadRelations });

  // 时间线相关的状态与计算已抽到 `hooks/useTimelineView.ts`：视图切换、日历偏移、
  // 时间流逝弹窗，以及 dayNodesSorted / timeGaps / dayNodes / 插入事件。
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
  // 第五批 ②：进入「演进提案」页时加载快照 / 审计 / 事件日志。
  //
  // **这是一处漏挂**：这三个 loader 原来只在"回滚成功后刷新"那一次被调用过，
  // 所以快照列表永远显示"暂无快照"（哪怕库里有 15 个），「回滚到此版本」按钮也就永远看不到
  // —— 用户实测反馈"到处都找不到回滚"，根因就在这里。
  // 切页 → 把中间工作区滚回顶部（否则页面顶部提示看不见）
  useEffect(() => {
    if (mainScrollRef.current) mainScrollRef.current.scrollTop = 0;
  }, [view]);

  // 2026-11-04：对话区"自动跟随最新内容"抽到 hooks/useFollowBottom.ts（第四刀）。
  useFollowBottom({
    view,
    genRequestId,
    messages,
    scrollRef: mainScrollRef,
  });

  // 2026-11-04：按视图拉数据的那些 useEffect 抽到 hooks/useViewEffects.ts（第五刀）。
  useViewEffects({
    view, currentStory, node,
    loadRequireApproval, loadSnapshots, loadAudit, loadEventLogs, loadDebug, loadChars,
    debugList, selectedDebug, setSelectedDebug, setPDebugId, setCtxWindow,
  });
  const dayDebug = node ? debugList.find((d) => d.node_id === node.id) ?? null : null;
  const pendingGroups = new Set(proposals.filter((p) => p.status === "pending").map((p) => p.group_id)).size;
    // 第五批 ③ 第 5 步：新建分支弹窗（根层渲染，与其它弹窗一致）
    // 2026-11-04：新建分支弹窗抽到 views/BranchDialogs.tsx（第三刀）。
    const branchDialogModal = (
      <CreateBranchDialog
        value={branchDialog}
        onChange={setBranchDialog}
        onCreate={() => void createBranch()}
        onCancel={() => setBranchDialog({ open: false, name: "", forkNodeId: null, busy: false })}
        timelineNodes={timelineNodes}
        branches={branches}
        counts={{ chars: chars.length, relations: relations.length, worlds: worlds.length }}
      />
    );

    // 第五批 ④：分支改名 / 备注弹窗（管理平行故事）
    // 2026-11-04：分支改名弹窗抽到 views/BranchDialogs.tsx（第三刀）。
    const branchEditModal = (
      <BranchEditDialog
        edit={branchEdit}
        onChange={(v) => setBranchEdit(v)}
        onSave={() => void saveBranchEdit()}
        onCancel={() => setBranchEdit(null)}
      />
    );


    // 2026-11-04：回滚差异预览弹窗抽到 views/SnapshotPreviewModal.tsx（App.tsx 第一刀）。
    const snapPreviewModal = snapPreview ? (
      <SnapshotPreviewModal
        preview={snapPreview}
        onClose={() => setSnapPreview(null)}
        onConfirm={(v) => void rollbackTo(v)}
      />
    ) : null;

  // 2026-11-04：合规弹窗抽到 views/ComplianceModal.tsx（App.tsx 第二刀）。
  // 仍然在根层渲染 —— 组件自己就是 position: fixed 的遮罩。
  const complianceModal = (
    <ComplianceModal
      open={complianceNotice}
      agreed={noticeAgreed}
      onAgree={setNoticeAgreed}
      onConfirm={() => void dismissComplianceNotice()}
    />
  );

  // ===== B 步：成片的 props 收成带类型的对象（`ComponentProps<typeof X>` 保证不漏不错） =====
  const storyInputProps: ComponentProps<typeof StoryInput> = {
    activeCleanupRef, aiRole, autoAnalyze,
    autoSummarizeToday, backfillSummaries, chars,
    constraints, ctxWindow, currentStory,
    dayDebug, directors, genRequestId,
    genRequestRef, input, lenLevel,
    loadChars, loadDebug,
    loadProposals, loadTimelineAndMessages, memoryCtx,
    loadPardons, loadRelations,
    myRole, navMode, node,
    recallPick, setRecallPick, recallDays, relations,
    sensitiveFilter, setAiRole, updateConstraints,
    setGenRequestId, setInput, setLenLevel,
    setMemoryCtx, setMessages, setMsg,
    setMyRole, setNavMode,
    setRecallDays, setShowRecallPicker, updateDirectors,
    msg,
  };
  const checkReportModalProps: ComponentProps<typeof CheckReportModal> = {
    applyCheckPatch, chars, relations,
    worlds, checkBoard, checkBusy,
    checkInput, checkItems, checkNote,
    checkPatch, checkPick, continueCheck,
    setCheckBoard, setCheckInput, setCheckPick,
  };
  const pluginsPageProps: ComponentProps<typeof PluginsPage> = {
    genPrefill, chars, cloudForm,
    cloudOverrides, cloudSaving, currentStory,
    genBusy,
    genList, genProbe, genProbing,
    genSel, genSummary, genUrl,
    loadCloudConfig, node,
    pluginKey, pluginState, runGenerate,
    setCloudForm, setCloudSaving,
    setGenProbe, setGenProbing, setGenSel,
    setGenUrl, setMsg: toastMsg, setPluginFlag,
  };
  const storyPanelProps: ComponentProps<typeof StoryPanel> = {
    headerEl: storyHeaderEl, branches, cancelEdit,
    chars, currentBranchId, currentStory,
    editText, editingId, loadBranches,
    loadChars, loadRelations, loadTimelineAndMessages,
    loadWorlds, messages, node,
    setBranchDialog, setBranchEdit, setCurrentBranchId,
    setEditText, setEditingId, setMsg,
    switchBranch, timelineNodes,
    setGapPrompt, timeGaps,
    removeNode, pardons,
  };
  const proposalsPageProps: ComponentProps<typeof ProposalsPage> = {
    setSnapPreview, analyzeInfo, auditLogs,
    chars, currentStory, eventLogs,
    loadAudit, loadChars, loadProposals,
    loadRelations, loadSnapshots, node,
    proposals, relations, requireApproval,
    selGroups, setAnalyzeInfo, setMsg: toastMsg,
    setRequireApproval, setSelGroups, snapshots,
    timelineNodes,
  };
  const paramsPageProps: ComponentProps<typeof ParamsPage> = {
    ctxWindow, currentStory, debugList,
    loadDebug, modelName,
    nRecentInput, node, pDebugId,
    setDebugList, setMsg: toastMsg, setPDebugId,
    setSelectedDebug,
    setView, timelineNodes,
  };
  const charsPageProps: ComponentProps<typeof CharsPage> = {
    charGraphRootId, chars, charsLoading,
    charUsage, currentStory, ensureEditBackup,
    loadChars, loadRelations,
    portraits, relations, runCheck,
    setCharGraphRootId, setMsg: toastMsg, setView,
  };
  const storiesPageProps: ComponentProps<typeof StoriesPage> = {
    createStory, enterStory, loadStories,
    newOrigin, newPrologue, newTitle,
    savePrologue: saveStoryPrologue, setNewPrologue, removeStory,
    setNewOrigin, setNewTitle, setShowNew,
    showNew, stories,
  };
  const directorPanelProps: ComponentProps<typeof DirectorPanel> = {
    chars, portraits, relations,
    currentStory, focusCharId, setFocusCharId,
    graphRootId, setGraphRootId, setView,
    setPDetail, ctxWindow, timelineNodes,
    node,
    reloadChars: () => (currentStory ? loadChars(currentStory.id) : Promise.resolve()),
  };
  const sidebarProps: ComponentProps<typeof Sidebar> = {
    complianceAck, currentStory, loadCloudConfig,
    loadComplianceAck, loadDebug,
    loadGenerators, navItems, pendingGroups,
    setGenProbe, setShowSettings, setView,
    view,
  };
  const timelinePageProps: ComponentProps<typeof TimelinePage> = {
    switchNode, setCalOffset, setMsg: toastMsg,
    setTimelineView, setView, branches,
    timelineView, calOffset, currentBranchId,
    currentStory, node, timelineNodes,
  };
  const portraitPageProps: ComponentProps<typeof PortraitPage> = {
    pDetail, setPDetail, portraits,
    loadPortraits, setMsg: toastMsg,
    stories, openGenPanel,
  };
  const worldPageProps: ComponentProps<typeof WorldPage> = {
    currentStory, ensureEditBackup, loadWorlds,
    runCheck, setMsg: toastMsg, worlds,
  };
  const compliancePageProps: ComponentProps<typeof CompliancePage> = {
    complianceAck, ackCompliance, checklistDone,
    setChecklistDone,
  };
  const settingsModalProps: ComponentProps<typeof SettingsModal> = {
    initial: { modelName, nRecent: nRecentInput, autoAnalyze }, onClose: () => setShowSettings(false), onOpenData: () => { setShowSettings(false); setView("temp"); loadTemp(); },
    onApplied: (v) => {
              setModelName(v.modelName);
              setNRecentInput(v.nRecent);
              setAutoAnalyze(v.autoAnalyze);
              if (v.ctxWindow > 0) setCtxWindow(v.ctxWindow);
            }, onMsg: toastMsg,
  };
  const exportModalProps: ComponentProps<typeof ExportModal> = {
    days: dayNodes, dayNoOf, onClose: () => setShowExport(false),
    onExport: doExport,
  };
  // 只在 `gapPrompt` 非空时渲染（见 JSX 的 `{gapPrompt && ...}`），所以这里用非空断言
  const insertEventModalProps: ComponentProps<typeof InsertEventModal> = {
    dayNodesSorted, gapPrompt: gapPrompt!, insertEventAtDay, timeGaps,
    setGapPrompt,
  };

    return (
      <>
      <div style={{ display: "flex", height: "100vh", fontFamily: "system-ui, sans-serif", background: T.win, color: T.text1 }}>
          <style>{`::-webkit-scrollbar{width:8px;height:8px}::-webkit-scrollbar-track{background:transparent}::-webkit-scrollbar-thumb{background:rgba(255,255,255,.14);border-radius:4px}::-webkit-scrollbar-thumb:hover{background:rgba(255,255,255,.24)}@keyframes dsh-blink{0%,49%{opacity:1}50%,100%{opacity:0}}button{background:#191c22;color:#a3a9b8;border:1px solid #2e333d;border-radius:6px;font-family:inherit;cursor:pointer}button:hover{color:#fff;border-color:#3d4451}button:disabled{opacity:.5;cursor:not-allowed}input,select,textarea{background:#191c22;color:#e8eaf0;border:1px solid #2e333d;border-radius:6px;font-family:inherit}input:focus,select:focus,textarea:focus{border-color:rgba(255,122,24,.55);outline:none}`}</style>
        {/* ① 左导航 */}
        <Sidebar {...sidebarProps} />

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
              {/* 2026-11-04：模型 / 导出 / 返回故事库 都是**打开故事之后**才成立的东西 ——
                  没开故事时不渲染（与导演监视器同一条口径：外壳留着，内容等打开再出）。 */}
              {currentStory && (
            <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 8 }}>
              <span style={{ display: "inline-flex", alignItems: "center", gap: 5, height: 26, padding: "0 10px", borderRadius: 13, background: T.panel2, border: `1px solid ${T.line}`, fontSize: 11.5, color: T.text2 }}>
                <span style={{ width: 6, height: 6, borderRadius: "50%", background: T.ok }} />{modelName ? `模型：${modelName}` : "模型未配置"}
              </span>
              <button onClick={() => { if (!currentStory) return; setShowExport(true); }} style={{ display: "inline-flex", alignItems: "center", gap: 5, height: 26, padding: "0 10px", borderRadius: 13, background: T.panel2, border: `1px solid ${T.line}`, fontSize: 11.5, color: T.text2, cursor: "pointer" }}>📥 导出</button>
              <button onClick={() => { setCurrentStory(null); setChars([]); setView("stories"); }} style={{ padding: "5px 12px", border: `1px solid ${T.line}`, borderRadius: 8, background: T.panel2, color: T.text2, cursor: "pointer", fontSize: 12 }}>← 返回故事库</button>
            </div>
              )}

          </div>

          {/* v3.5.5：剧情页头部的宿主节点 —— 与标题行同层级、在滚动容器外面 */}
          {/* v3.5.5：剧情页头部的宿主节点 —— 与标题行同层级、在滚动容器外面。
              注意：**常驻**（不加 `view === "story"` 条件）。
              否则第一次进入剧情页时 `storyHeaderEl` 还是 null → 头部要等下一帧才出现（闪一下）。 */}
          <div ref={attachStoryHeader} style={{ flexShrink: 0, padding: "0 14px" }} />

          <div ref={mainScrollRef} style={{ flex: 1, overflowY: "auto", padding: "20px 14px 12px" }}>

            {/* 故事列表（仅点击左侧"故事列表"时显示；首页为空白）
                宽度自适应：不再限宽，随中间工作区撑满（左右留白各 24px，来自上一层容器 padding） */}
        {view === "stories" && (
          <StoriesPage {...storiesPageProps} />
        )}



        {view === "chars" && (
          <CharsPage {...charsPageProps} />
        )}
        {view === "world" && (
          <WorldPage {...worldPageProps} />
        )}
        {view === "proposals" && (
          <ProposalsPage {...proposalsPageProps} />
        )}

        {view === "temp" && (
          <TempPage tempFiles={tempFiles} loadTemp={loadTemp} cleanTemp={cleanTemp} />
        )}

        {view === "timeline" && (
          <TimelinePage {...timelinePageProps} />
        )}

        {/* 历史章节（2026-10-30 阶段①）：只读聚合；点「打开」跳回剧情区 */}
        {view === "chapters" && (
          <ChaptersPage
            currentStory={currentStory}
            branchId={currentBranchId}
            nodes={timelineNodes}
            openChapterNode={(nodeId) => {
              if (!currentStory) return;
              setView("story");
              void loadTimelineAndMessages(currentStory.id, nodeId, currentBranchId);
            }}
          />
        )}

        {view === "portrait" && (
          <PortraitPage {...portraitPageProps} />
        )}
        {import.meta.env.DEV && view === "devtools" && (
          <DevToolsPage
            currentStory={currentStory}
            nodes={timelineNodes}
            branches={branches}
            currentBranchId={currentBranchId}
          />
        )}
        {view === "params" && (
          <ParamsPage {...paramsPageProps} />
        )}
        {view === "plugins" && (
          <PluginsPage {...pluginsPageProps} />
        )}

        {view === "compliance" && (
          <CompliancePage {...compliancePageProps} />
        )}

        {view === "story" && (
          <StoryPanel {...storyPanelProps} />
        )}
          </div>

          {/* 第二批 · ② 检查：报告弹窗（C2 抽到 views/CheckReportModal.tsx） */}
          {checkBoard && (
            <CheckReportModal {...checkReportModalProps} />
          )}

          {/* C 子项：在"时间流逝"区间里插入事件（C2 抽到 views/InsertEventModal.tsx） */}
          {gapPrompt && (
            <InsertEventModal {...insertEventModalProps} />
          )}

          {/* 底部输入台（仅剧情 view） */}
        {view === "story" && (
          <StoryInput {...storyInputProps} />
        )}
        </div>

        {/* ③ 右侧信息面板（导演监视器 / 素材库）—— 2026-10-20 抽到 views/DirectorPanel.tsx */}
        <DirectorPanel {...directorPanelProps} />

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
                    onClick={() => {
                      // 2026-11-02：改成**多选**（连续几天 = 范围）；最近三天/未来天置灰不给选
                      if (!d.eligible) {
                        setMsg(`🔍「${d.date_label}」在最近三天内或还没发生，不需要回顾`);
                        return;
                      }
                      setRecallPick((prev) =>
                        prev.includes(d.node_id)
                          ? prev.filter((x) => x !== d.node_id)
                          : [...prev, d.node_id].sort((a, b) => a - b)
                      );
                    }}
                    style={{ display: "block", width: "100%", textAlign: "left", marginBottom: 6, padding: "9px 11px", borderRadius: 9, border: `1px solid ${recallPick.includes(d.node_id) ? T.accent : T.line}`, background: recallPick.includes(d.node_id) ? T.accentSoft : T.panel2, cursor: "pointer", fontFamily: "inherit" }}>
                    <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                      <span style={{ fontSize: 12.5, fontWeight: 600, color: recallPick.includes(d.node_id) ? T.accent : (d.eligible ? T.text1 : T.text3) }}>{d.date_label}</span>
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
                **可以连点几天 = 范围**（最近三天和还没发生的天置灰，不需要回顾）。回顾带进去的是**当天聊过的原始对话**（不是压缩后的摘要）。
                选了之后只在**下一次发送**生效。不选的话，软件会自己按你输入里的「第一次 / 那次 / 第 N 天」等词判断要不要回顾。
              </div>
            </div>
          </div>
        )}
        {/* ⚙️ 设置弹窗（2026-10-20 抽到 views/SettingsModal.tsx） */}
        {showSettings && (
          <SettingsModal {...settingsModalProps} />
        )}

        {showExport && (
          <ExportModal {...exportModalProps} />
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
