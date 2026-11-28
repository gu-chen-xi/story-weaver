import { useCallback, useEffect, useRef, useState, type ComponentProps, type Dispatch, type SetStateAction } from "react";
import type { AppView } from "./types";
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
import { RecallPickerModal } from "./views/RecallPickerModal";
import { StoriesPage } from "./views/StoriesPage";
import { HomePage } from "./views/HomePage";
import { Sidebar } from "./views/Sidebar";
import { AppHeader } from "./views/AppHeader";
import { DirectorPanel } from "./views/DirectorPanel";
import { ExportModal } from "./views/ExportModal";
import { SettingsModal } from "./views/SettingsModal";
import { CheckReportModal } from "./views/CheckReportModal";
import { InsertEventModal } from "./views/InsertEventModal";
import { ComplianceModal } from "./views/ComplianceModal";
import { BranchEditDialog, CreateBranchDialog } from "./views/BranchDialogs";
import { useStorySession } from "./hooks/useStorySession";
import { useFollowBottom } from "./hooks/useFollowBottom";
import { useRoleDefaults } from "./hooks/useRoleDefaults";
import { useStoryActions } from "./hooks/useStoryActions";
import { useSoftScrollbar } from "./hooks/useSoftScrollbar";
import { useViewEffects } from "./hooks/useViewEffects";
import { useConfirm } from "./confirm";
import { useToast } from "./toast";
import { buildStoryNavItems } from "./lib/navigation";

function App() {
  const confirm = useConfirm();
  const toast = useToast();
  const [msg, setMsg] = useState("");
  const [focusCharId, setFocusCharId] = useState<number | null>(null);

  const toastMsg: Dispatch<SetStateAction<string>> = useCallback((v) => {
    const s = typeof v === "function" ? v("") : v;
    if (s && s.trim()) toast(s);
  }, [toast]);

  const session = useStorySession({ setFocusCharId, setMsg });
  const { stories, setCurrentStory, currentStory, setShowNew, showNew, setNewTitle, newTitle, newOrigin, setNewOrigin, newPrologue, setNewPrologue, setDebugList, debugList, selectedDebug, setSelectedDebug, setChars, chars, charUsage, charsLoading, setRecallPick, recallPick, recallDays, setRecallDays, worlds, node, branches, currentBranchId, setCurrentBranchId, setBranchDialog, branchDialog, branchEdit, setBranchEdit, timelineNodes, setMessages, messages, relations, proposals, setPluginState, pluginState, genList, genSel, setGenSel, setCloudForm, cloudForm, loadCloudConfig, loadGenerators, setRequireApproval, requireApproval, loadRequireApproval, loadProposals, pardons, loadPardons, snapshots, backupTakenRef, auditLogs, eventLogs, loadSnapshots, loadAudit, loadEventLogs, loadStories, loadDebug, backfillSummaries, autoSummarizeToday, loadBranches, switchBranch, createBranch, saveBranchEdit, loadTimelineAndMessages, switchNode, loadChars, loadWorlds, loadRelations, ensureEditBackup , removeNode} = session;
  const mainScrollRef = useRef<HTMLDivElement | null>(null);

  const [view, setView] = useState<AppView>("home");
  const sensitiveFilter = true;

  const {
    navMode, setNavMode, myRole, setMyRole, aiRole, setAiRole,
    constraints, updateConstraints, lenLevel, setLenLevel,
    memoryCtx, setMemoryCtx, showRecallPicker, setShowRecallPicker,
    directors, updateDirectors, showExport, setShowExport,
  } = useStoryInputPrefs();
  useEffect(() => { void loadPerformLexicon(); }, []);

  useEffect(() => { loadPortraits(); }, []);
  const navItems = buildStoryNavItems(() => {
    setCurrentStory(null);
    setChars([]);
    setView("stories");
  });

  const [genRequestId, setGenRequestId] = useState<string | null>(null);
  const genRequestRef = useRef<string | null>(null);   // 给事件回调读（避免闭包拿到旧 state）
  const activeCleanupRef = useRef<(() => void) | null>(null);

  const {
    complianceAck,
    complianceNotice,
    noticeAgreed, setNoticeAgreed,
    checklistDone, setChecklistDone,
    loadComplianceAck, dismissComplianceNotice, ackCompliance,
  } = useCompliance({ setMsg: toastMsg });

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

  const { createStory, removeStory, saveStoryPrologue, enterStory, doExport } = useStoryActions({
    currentStory, node, dayNodes, dayNoOf, newTitle, newOrigin, newPrologue,
    setShowNew, setNewTitle, setNewPrologue, setShowExport, setCurrentStory,
    setDebugList, setSelectedDebug, setFocusCharId, setView, setMsg,
    confirm, loadStories, loadBranches, loadChars, loadWorlds, loadRelations,
    loadTimelineAndMessages, loadProposals,
  });

  const { tempFiles, loadTemp, cleanTemp } = useTempFiles({ setMsg: toastMsg });

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
    currentStory, setMsg: toastMsg, setView,
    pluginState, setPluginState, genList, genSel, cloudForm, loadCloudConfig, loadPortraits, loadGenerators,
  });

  const {
    showSettings, setShowSettings, modelName, setModelName,
    ctxWindow, setCtxWindow, nRecentInput, setNRecentInput,
    autoAnalyze, setAutoAnalyze, loadSettings,
  } = useAppSettings({
    setPluginState,
    getPluginState: () => pluginState,
    pluginKey,
  });

  useEffect(() => {
    loadStories();
    loadSettings();
    void loadComplianceAck();
  }, []);

  useEffect(() => () => {
    try { activeCleanupRef.current?.(); } catch { /* 忽略 */ }
    activeCleanupRef.current = null;
  }, []);

  useRoleDefaults({
    currentStory, chars, charsLoading, myRole, aiRole, setMyRole, setAiRole,
    setMsg, backupTakenRef,
  });

  const {
    checkBoard, setCheckBoard, checkBusy, checkItems,
    checkPatch, checkPick, setCheckPick,
    checkInput, setCheckInput, checkNote,
    runCheck, continueCheck, applyCheckPatch,
  } = useCheckPatch({ currentStory, chars, relations, worlds, setMsg: toastMsg, loadChars, loadWorlds, loadRelations });

  useEffect(() => {
    if (mainScrollRef.current) mainScrollRef.current.scrollTop = 0;
  }, [view]);

  useFollowBottom({
    view,
    genRequestId,
    messages,
    scrollRef: mainScrollRef,
  });

  useSoftScrollbar(mainScrollRef);

  useViewEffects({
    view, currentStory, node,
    loadRequireApproval, loadSnapshots, loadAudit, loadEventLogs, loadDebug, loadChars, loadStories,
    debugList, selectedDebug, setSelectedDebug, setPDebugId, setCtxWindow,
  });
  const dayDebug = node ? debugList.find((d) => d.node_id === node.id) ?? null : null;
  const pendingGroups = new Set(proposals.filter((p) => p.status === "pending").map((p) => p.group_id)).size;
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

    const branchEditModal = (
      <BranchEditDialog
        edit={branchEdit}
        onChange={(v) => setBranchEdit(v)}
        onSave={() => void saveBranchEdit()}
        onCancel={() => setBranchEdit(null)}
      />
    );

  const complianceModal = (
    <ComplianceModal
      open={complianceNotice}
      agreed={noticeAgreed}
      onAgree={setNoticeAgreed}
      onConfirm={() => void dismissComplianceNotice()}
    />
  );

  const storyInputProps: ComponentProps<typeof StoryInput> = {
    activeCleanupRef, aiRole, autoAnalyze,
    autoSummarizeToday, backfillSummaries, chars,
    constraints, ctxWindow, currentStory,
    dayDebug, directors, genRequestId,
    genRequestRef, lenLevel,
    loadChars, loadDebug,
    loadProposals, loadTimelineAndMessages, memoryCtx,
    loadPardons, loadRelations,
    myRole, navMode, node,
    recallPick, setRecallPick, recallDays, relations,
    sensitiveFilter, setAiRole, updateConstraints,
    setGenRequestId, setLenLevel,
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
    headerEl: storyHeaderEl, branches,
    chars, currentBranchId, currentStory,
    loadBranches,
    loadChars, loadProposals, loadRelations, loadTimelineAndMessages,
    loadWorlds, messages, node,
    setBranchDialog, setBranchEdit, setCurrentBranchId,
    setMsg,
    switchBranch, timelineNodes,
    setGapPrompt, timeGaps,
    removeNode, pardons,
  };
  const proposalsPageProps: ComponentProps<typeof ProposalsPage> = {
    auditLogs,
    chars, currentStory, eventLogs,
    loadAudit, loadChars, loadProposals,
    loadRelations, loadSnapshots, loadWorlds, node,
    proposals, relations, requireApproval,
    setMsg: toastMsg, setRequireApproval, snapshots,
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
    chars, charsLoading,
    charUsage, currentStory, ensureEditBackup,
    loadChars, loadRelations,
    portraits, relations, runCheck,
    setMsg: toastMsg, setView,
  };
  const homePageProps: ComponentProps<typeof HomePage> = {
    stories,
    onContinue: (s) => enterStory(s, "story"),
    onNew: () => { setView("stories"); setShowNew(true); },
    onAll: () => { setView("stories"); setShowNew(false); },
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
    setView,
    setPDetail, ctxWindow, timelineNodes,
    node,
    reloadChars: () => (currentStory ? loadChars(currentStory.id) : Promise.resolve()),
  };
  const sidebarProps: ComponentProps<typeof Sidebar> = {
    complianceAck, currentStory, loadCloudConfig,
    loadComplianceAck, loadDebug,
    loadGenerators, loadProposals, navItems, pendingGroups,
    goHome: () => { setCurrentStory(null); setChars([]); setView("home"); },
    setGenProbe, setShowSettings, setView,
    view,
  };
  const timelinePageProps: ComponentProps<typeof TimelinePage> = {
    switchNode, setCalOffset,
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
  const insertEventModalProps: ComponentProps<typeof InsertEventModal> = {
    dayNodesSorted, gapPrompt: gapPrompt!, insertEventAtDay, timeGaps,
    setGapPrompt,
  };

    return (
      <>
      <div style={{ display: "flex", height: "100vh", fontFamily: "system-ui, sans-serif", background: T.win, color: T.text1 }}>
        <Sidebar {...sidebarProps} />

        <div style={{ flex: 1, display: "flex", flexDirection: "column", minWidth: 0, background: T.win }}>
          <AppHeader
            currentStory={currentStory}
            view={view}
            timelineNodeCount={timelineNodes.length}
            messageCount={messages.length}
            storyCount={stories.length}
            modelName={modelName}
            onExport={() => setShowExport(true)}
            onBackToStories={() => { setCurrentStory(null); setChars([]); setView("stories"); }}
          />

          {/* v3.5.5：剧情页头部的宿主节点 —— 与标题行同层级、在滚动容器外面。
              注意：**常驻**（不加 `view === "story"` 条件）。
              否则第一次进入剧情页时 `storyHeaderEl` 还是 null → 头部要等下一帧才出现（闪一下）。 */}
          <div ref={attachStoryHeader} style={{ flexShrink: 0, padding: "0 14px" }} />

          <div ref={mainScrollRef} className="dw-soft" style={{ flex: 1, overflowY: "auto", padding: "20px 14px 22px" }}>

            {/* 首页 = 启动台（2026-11-19，`views/HomePage.tsx`）：问候 + 继续创作 + 最近编辑；
                数据只用 stories（后端已按 updated_at DESC），管理动作仍在「故事列表」页 */}
        {view === "home" && (
          <HomePage {...homePageProps} />
        )}
        {/* 故事列表（仅点击左侧"故事列表"时显示）
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

          {checkBoard && (
            <CheckReportModal {...checkReportModalProps} />
          )}

          {gapPrompt && (
            <InsertEventModal {...insertEventModalProps} />
          )}

        {view === "story" && (
          <StoryInput {...storyInputProps} />
        )}
        </div>

        <DirectorPanel {...directorPanelProps} />

        {showRecallPicker && (
          <RecallPickerModal
            days={recallDays}
            picked={recallPick}
            onToggle={(id) => setRecallPick((prev) => (prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id].sort((a, b) => a - b)))}
            onBlocked={(label) => setMsg(`🔍「${label}」在最近三天内或还没发生，不需要回顾`)}
            onClose={() => setShowRecallPicker(false)}
          />
        )}
        {showSettings && (
          <SettingsModal {...settingsModalProps} />
        )}

        {showExport && (
          <ExportModal {...exportModalProps} />
        )}
      </div>
      {complianceModal}
      {branchDialogModal}
      {branchEditModal}
      </>
    );
}
export default App;
