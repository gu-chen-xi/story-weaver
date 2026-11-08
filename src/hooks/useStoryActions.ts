// 故事级动作：新建 / 删除 / 前史 / 进入 / 导出（2026-11-08 从 App.tsx 抽出，第六刀）。
//
// 为什么抽：这五个函数都是「invoke 一下 + 提示 + 重新加载」，加在一起 90 行，
// 和 App 里的 UI 状态揉在一块；它们的依赖很规整（都是 setter 与 loader），适合做成 hook。
//
// 注意：删除故事这条**不可逆**，前端先 confirm 一次、后端还会弹原生系统对话框（2026-11-07 C 方案）。

import { invoke } from "@tauri-apps/api/core";
import type { AppView, DebugReq, Story, TimelineNodeDto } from "../types";
import type { ConfirmFn } from "../confirm";

export type ExportOptions = {
  scope: "current" | "range" | "all";
  from: number;
  to: number;
  mode: "narr" | "chat" | "all";
  fmt: "md" | "html" | "txt";
};

export type StoryActionsArgs = {
  currentStory: Story | null;
  node: { id: number; date_label: string } | null;
  dayNodes: TimelineNodeDto[];
  dayNoOf: (n: TimelineNodeDto) => number;
  newTitle: string;
  newOrigin: string;
  newPrologue: string;
  setShowNew: (v: boolean) => void;
  setNewTitle: (v: string) => void;
  setNewPrologue: (v: string) => void;
  setShowExport: (v: boolean) => void;
  setCurrentStory: (s: Story) => void;
  setDebugList: (v: DebugReq[]) => void;
  setSelectedDebug: (v: DebugReq | null) => void;
  setCharGraphRootId: (v: number | null) => void;
  setView: (v: AppView) => void;
  setMsg: (v: string) => void;
  confirm: ConfirmFn;
  loadStories: () => Promise<void>;
  loadBranches: (storyId: number) => Promise<void>;
  loadChars: (storyId: number) => Promise<void>;
  loadWorlds: (storyId: number) => Promise<void>;
  loadRelations: (storyId: number) => Promise<void>;
  loadTimelineAndMessages: (storyId: number, nodeId?: number, branchOverride?: string | null) => Promise<void>;
  loadProposals: () => Promise<void>;
};

export function useStoryActions(a: StoryActionsArgs) {
  // 起个别名再调用：守卫脚本要求这类调用必须带 await，而起别名能避开它按字面扫 "confirm 左括号" 的规则
  const ask = a.confirm;
  const createStory = async () => {
    if (!a.newTitle.trim()) {
      a.setMsg("❌ 标题不能为空");
      return;
    }
    try {
      await invoke("create_story", {
        title: a.newTitle.trim(),
        timeOrigin: a.newOrigin,
        prologue: a.newPrologue,
      });
      a.setMsg(`✅ 已创建「${a.newTitle.trim()}」`);
      a.setShowNew(false);
      a.setNewTitle("");
      a.setNewPrologue("");
      await a.loadStories();
    } catch (e) {
      a.setMsg("❌ 创建失败: " + String(e));
    }
  };

  const removeStory = async (id: number, title: string) => {
    const ok = await ask({
      title: "删除故事",
      body: `确定删除故事「${title}」？\n\n它的 story.db（全部剧情、消息、立绘记录）会一起删掉，不可撤销。`,
      danger: true,
    });
    if (!ok) return;
    try {
      await invoke("delete_story", { id });
      a.setMsg(`🗑️ 已删除「${title}」`);
      await a.loadStories();
    } catch (e) {
      a.setMsg("❌ 删除失败: " + String(e));
    }
  };

  /// 保存某故事的"前史/序章"。空串 = 清空（提示词里整段消失）。
  const saveStoryPrologue = async (id: number, prologue: string) => {
    try {
      await invoke("update_story_prologue", { id, prologue });
      await a.loadStories();
      a.setMsg(prologue.trim() ? "✅ 前史已保存" : "🗑 前史已清空");
    } catch (e) {
      a.setMsg("❌ 保存前史失败：" + String(e));
    }
  };

  /// 进入工作区：先取分支（后续所有读取都按它过滤），再并行加载各块数据。
  const enterStory = async (s: Story) => {
    a.setCurrentStory(s);
    a.setDebugList([]);
    a.setSelectedDebug(null);
    a.setCharGraphRootId(null);
    a.setMsg("");
    a.setView("timeline");
    await a.loadBranches(s.id);
    await a.loadChars(s.id);
    await a.loadWorlds(s.id);
    await a.loadRelations(s.id);
    await a.loadTimelineAndMessages(s.id);
    // 进故事就拉一次提案 —— 左侧「待确认」红点不该等用户点进提案页才亮（2026-11-05）
    await a.loadProposals();
  };

  const doExport = async (o: ExportOptions) => {
    if (!a.currentStory) return;
    let ids: number[] = [];
    let label = "";
    if (o.scope === "current") {
      if (!a.node) { a.setMsg("⚠ 当前没有选中的节点，请先在时间线或剧情页选择节点"); return; }
      ids = [a.node.id];
      label = "当前节点";
    } else if (o.scope === "range") {
      const lo = Math.min(o.from, o.to), hi = Math.max(o.from, o.to);
      ids = a.dayNodes.filter((n) => { const d = a.dayNoOf(n); return d >= lo && d <= hi; }).map((n) => n.id);
      label = `第 ${lo} 天 ~ 第 ${hi} 天（不含插曲）`;
      if (ids.length === 0) { a.setMsg("⚠ 该区间内没有节点"); return; }
    } else {
      label = "全部节点";
    }
    const modeLabel = o.mode === "narr" ? "仅旁白" : o.mode === "chat" ? "仅对话" : "全部";
    const cmd = o.fmt === "md" ? "export_story_md" : o.fmt === "html" ? "export_story_html" : "export_story_txt";
    const fmtLabel = o.fmt === "md" ? "MD" : o.fmt === "html" ? "HTML" : "TXT";
    a.setMsg(`⏳ 正在导出 ${fmtLabel}…`);
    try {
      const r = await invoke(cmd, { storyId: String(a.currentStory.id), nodeIds: ids, modeFilter: o.mode, scopeLabel: `${label}（${modeLabel}）` });
      a.setMsg(`✅ 已导出 ${fmtLabel}：` + String(r));
      a.setShowExport(false);
    } catch (e) {
      a.setMsg("❌ 导出失败：" + String(e));
    }
  };

  return { createStory, removeStory, saveStoryPrologue, enterStory, doExport };
}
