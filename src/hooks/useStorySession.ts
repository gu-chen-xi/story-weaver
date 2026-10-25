// 故事会话：数据加载 + 分支/节点操作（从 App.tsx 抽出，2026-10-17）。
//
// 为什么抽这一刀：这些 state 与加载器是 App 里最大的一坨（29 个 state + 20 多个异步加载器），
// 而且彼此强耦合（加载时间线要带分支、切分支要重载全部）—— 留在 App 里会一直长。
// 页面组件只接收它们需要的值/回调，不再直接碰这些内部状态。
//
// 参数（deps）只保留 3~5 个跨域回调：全局提示、视图跳转、少数几个页面的局部 setter。

import { useRef, useState, type Dispatch, type SetStateAction } from "react";
import { PLUGIN_REGISTRY } from "../constants";
import { CloudForm, GenDescriptor } from "../plugins";
import { AuditLog, BranchDto, Character, CharacterUsage, DebugReq, EvalT, EventLogItem, Proposal, Relation, Snapshot, Story, TimelineNodeDto, WorldSetting } from "../types";
import { invoke } from "@tauri-apps/api/core";
import { useConfirm } from "../confirm";

export type StorySessionDeps = {
  setFocusCharId: Dispatch<SetStateAction<number | null>>;
  setMsg: Dispatch<SetStateAction<string>>;
};

export function useStorySession(deps: StorySessionDeps) {
  const confirm = useConfirm();
  const { setFocusCharId, setMsg } = deps;

  const [stories, setStories] = useState<Story[]>([]);
  const [currentStory, setCurrentStory] = useState<Story | null>(null);
  const [showNew, setShowNew] = useState(false);
  const [newTitle, setNewTitle] = useState("");
  const [newOrigin, setNewOrigin] = useState("2026-01-01");
  // v3.3：前史/序章（建故事时填，之后可在故事卡片里改）
  const [newPrologue, setNewPrologue] = useState("");
  const [debugList, setDebugList] = useState<DebugReq[]>([]);
  const [selectedDebug, setSelectedDebug] = useState<DebugReq | null>(null);
  const [chars, setChars] = useState<Character[]>([]);
  const [charUsage, setCharUsage] = useState<CharacterUsage[]>([]);
  const [charsLoading, setCharsLoading] = useState(false);
  const [recallDay, setRecallDay] = useState<number | null>(null);
  const [recallDays, setRecallDays] = useState<{ node_id: number; date_label: string; day: number | null; msg_count: number; preview: string; has_summary: boolean; tags?: string[] }[]>([]);
  const [worlds, setWorlds] = useState<WorldSetting[]>([]);
  const [node, setNode] = useState<{ id: number; date_label: string } | null>(null);
  const [branches, setBranches] = useState<BranchDto[]>([]);
  const [currentBranchId, setCurrentBranchId] = useState<string | null>(null);
  const [branchDialog, setBranchDialog] = useState<{ open: boolean; name: string; forkNodeId: number | null; busy: boolean }>(
    { open: false, name: "", forkNodeId: null, busy: false }
  );
  const [branchEdit, setBranchEdit] = useState<{ id: string; name: string; note: string; busy: boolean } | null>(null);
  const [timelineNodes, setTimelineNodes] = useState<TimelineNodeDto[]>([]);
  const [messages, setMessages] = useState<{ id: number; role: string; content: string; mode: string; created_at: string }[]>([]);
  const [relations, setRelations] = useState<Relation[]>([]);
  const [proposals, setProposals] = useState<Proposal[]>([]);
  const [evals, setEvals] = useState<EvalT[]>([]);
  const [evalBusy, setEvalBusy] = useState(false);
  const [pluginState, setPluginState] = useState<Record<string, { enabled: boolean; panelOpen: boolean }>>(
    Object.fromEntries(PLUGIN_REGISTRY.map((d) => [d.id, { enabled: false, panelOpen: false }]))
  );
  const [genList, setGenList] = useState<GenDescriptor[]>([]);
  const [genSel, setGenSel] = useState("local_sdwebui");
  const [cloudForm, setCloudForm] = useState<CloudForm>({
    base_url: "", model: "", url: "", body_template: "", response_path: "", openai_response_path: "",
    auth_style: "bearer", extra_headers: "", api_key: "", has_api_key: false,
  });
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
        openai_response_path: String(c.openai_response_path ?? ""),
        auth_style: String(c.auth_style || "bearer"),
        extra_headers: String(c.extra_headers ?? ""),
        api_key: "",                       // 不回显明文
        has_api_key: Boolean(c.has_api_key),
      }));
    } catch (e) { console.error("读云端配置失败：", e); }
  };
  const loadGenerators = async () => {
    try {
      const list = await invoke<GenDescriptor[]>("generator_list");
      setGenList(list);
      if (list.length > 0 && !list.some((g) => g.id === genSel)) setGenSel(list[0].id);
    } catch (e) { console.error("载入生成通道失败：", e); }
  };
  const [requireApproval, setRequireApproval] = useState(true);
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
  const loadEvals = async () => {
    if (!currentStory) { setEvals([]); return; }
    try {
      const list = await invoke<EvalT[]>("list_reply_evaluations", { storyId: String(currentStory.id), limit: 20 });
      setEvals(list);
    } catch (e) { console.error("载入评估失败：", e); }
  };
  const [snapshots, setSnapshots] = useState<Snapshot[]>([]);
  const backupTakenRef = useRef(false);
  const [auditLogs, setAuditLogs] = useState<AuditLog[]>([]);
  const [eventLogs, setEventLogs] = useState<EventLogItem[]>([]);
  const loadSnapshots = async () => {
    try { setSnapshots(await invoke<Snapshot[]>("list_snapshots", { storyId: String(currentStory?.id) })); } catch (e) { console.error(e); }
  };
  const loadAudit = async () => {
    try { setAuditLogs(await invoke<AuditLog[]>("list_audit_logs", { storyId: String(currentStory?.id) })); } catch (e) { console.error(e); }
  };
  const loadEventLogs = async () => {
    try { setEventLogs(await invoke<EventLogItem[]>("list_event_logs", { storyId: String(currentStory?.id) })); } catch (e) { console.error(e); }
  };
  const loadStories = async () => {
    try {
      const list = await invoke<Story[]>("list_stories");
      setStories(list);
    } catch (e) {
      setMsg("❌ 加载故事失败: " + String(e));
    }
  };
  const loadDebug = async () => {
    if (!currentStory) return;
    try {
      const list = await invoke<DebugReq[]>("list_debug_requests", { storyId: String(currentStory.id) });
      setDebugList(list);
    } catch (e) {
      setMsg("❌ 加载设定版本失败: " + String(e));
    }
  };
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
  const autoSummarizeToday = async (storyId: string, nodeId: number) => {
    try {
      const made = await invoke<string | null>("maybe_auto_day_summary", { storyId, nodeId, minReplies: SUMMARY_MIN_REPLIES });
      // ⑥ 阶段摘要（C 层）：**每次回复完成都试一次**（不是等日摘要生成才试）——
      // 它自己判断"攒够 10 天没"，不够就返回 null，代价只是一次本地查询。
      void invoke<string | null>("maybe_auto_phase_summary", { storyId })
        .then((ph) => { if (ph) setMsg("🧾 已生成阶段摘要（" + ph.slice(0, 20) + "…）"); })
        .catch(() => { /* 静默：没有 API Key / 网络失败都不打扰用户 */ });
      if (made) {
        setMsg(`📝 ${made}，已自动生成当天日摘要`);
        await refreshTimelineNodes(storyId);
      }
    } catch (e) {
      setMsg("⚠ 当天日摘要生成失败：" + String(e));
      console.error(e);
    }
  };
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
      // 用户点了**另一条线上的节点**时，必须跟着切过去。
      //
      // 原来的写法是"不在当前分支就退回当前分支最后一个节点" —— 于是点击被**静默忽略**：
      // 当前节点没变 → 接着点「下一天」就建到了旧分支（用户实测：从「从7分支」或嵌套子分支
      // 新建下一天，全跑到第一个分支里去了）。
      let eff = effectiveBranch ?? null;
      if (wanted && nodeId != null) {
        const wb = wanted.branch_id ?? null;
        if (wb !== eff) {
          eff = wb;
          setCurrentBranchId(wb);
          // 后端那份"当前分支"也要跟上（它决定默认落在哪个节点）
          await invoke("set_current_branch", { storyId: String(storyId), branchId: wb }).catch(() => {});
        }
      }
      // 空分支（刚建好、还没有自己的节点）要落在**分叉点**上，而不是全库最后一个节点。
      //
      // 为什么：后者可能属于别的分支 → 点「下一天」时 afterNodeId 指向别人，
      // 后端按节点推导分支就会把新节点建到那条分支里去（用户实测：从「从7分支」新建下一天，
      // 跑到了第一个分支）。
      let fallback = candidates[candidates.length - 1];
      if (eff != null && mine.length === 0) {
        const forkId = branches.find((b) => b.id === eff)?.fork_node_id;
        const forkNode = forkId != null ? nodes.find((n) => n.id === forkId) : undefined;
        if (forkNode) fallback = forkNode;
      }
      const target = (wanted && (wanted.branch_id ?? null) === eff)
        ? wanted
        : fallback;
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
  const removeNode = async (nodeId: number, label: string) => {
    if (!currentStory) return;
    if (!(await confirm({
      title: "删除节点",
      body: `删除「${label}」节点及其所有消息？\n\n不可撤销（后面的节点会接回上一层，不会跟着消失）。`,
      danger: true,
    }))) return;
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

  return {
    setStories,
    stories,
    setCurrentStory,
    currentStory,
    setShowNew,
    showNew,
    setNewTitle,
    newTitle,
    newOrigin,
    setNewOrigin,
    newPrologue,
    setNewPrologue,
    setDebugList,
    debugList,
    selectedDebug,
    setSelectedDebug,
    setChars,
    chars,
    setCharUsage,
    charUsage,
    charsLoading,
    setCharsLoading,
    setRecallDay,
    recallDay,
    recallDays,
    setRecallDays,
    worlds,
    setWorlds,
    setNode,
    node,
    branches,
    setBranches,
    currentBranchId,
    setCurrentBranchId,
    setBranchDialog,
    branchDialog,
    branchEdit,
    setBranchEdit,
    timelineNodes,
    setTimelineNodes,
    setMessages,
    messages,
    setRelations,
    relations,
    setProposals,
    proposals,
    setEvals,
    evals,
    setEvalBusy,
    evalBusy,
    setPluginState,
    pluginState,
    genList,
    setGenList,
    genSel,
    setGenSel,
    setCloudForm,
    cloudForm,
    loadCloudConfig,
    loadGenerators,
    setRequireApproval,
    requireApproval,
    loadRequireApproval,
    loadProposals,
    loadEvals,
    snapshots,
    setSnapshots,
    backupTakenRef,
    setAuditLogs,
    auditLogs,
    eventLogs,
    setEventLogs,
    loadSnapshots,
    loadAudit,
    loadEventLogs,
    loadStories,
    loadDebug,
    SUMMARY_MIN_REPLIES,
    refreshTimelineNodes,
    backfillSummaries,
    autoSummarizeToday,
    loadBranches,
    switchBranch,
    createBranch,
    loadTimelineAndMessages,
    switchNode,
    removeNode,
    loadChars,
    loadWorlds,
    loadRelations,
    ensureEditBackup,
  };
}
