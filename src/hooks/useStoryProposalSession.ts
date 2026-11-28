import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { expectedError, reportError } from "../lib/errors";
import type { AuditLog, EventLogItem, Proposal, Snapshot, Story } from "../types";

export function useStoryProposalSession(currentStory: Story | null) {
  const [proposals, setProposals] = useState<Proposal[]>([]);
  const [requireApproval, setRequireApproval] = useState(true);
  const [snapshots, setSnapshots] = useState<Snapshot[]>([]);
  const [auditLogs, setAuditLogs] = useState<AuditLog[]>([]);
  const [eventLogs, setEventLogs] = useState<EventLogItem[]>([]);

  /// 解析本次要用的 storyId。
  ///
  /// **必须支持显式传参**：进故事那一刻 `currentStory` 还是旧值（`setCurrentStory` 只是排队更新
  /// state，本渲染里的闭包拿不到新值）。以前这里直接 `String(currentStory?.id)`，
  /// 首页进故事时得到的是字符串 `"undefined"`，后端就去开 `projects/undefined/story.db`，
  /// 报 `(code: 14) unable to open database file` —— 点**任何**故事都中招。
  /// 见错误合集 E-UI-35。写法与 `useStorySession.loadPardons` 保持一致。
  const sidOf = (arg?: number): number | null => arg ?? currentStory?.id ?? null;

  const loadRequireApproval = async () => {
    try {
      setRequireApproval(await invoke<boolean>("get_evolution_require_approval"));
    } catch (e) {
      // 读不到审批开关时保持安全默认值 true；这是明确的降级，不需要打扰用户。
      expectedError("读取演进审批开关失败，按默认 true 降级", e);
    }
  };

  const loadEventLogs = async (storyIdArg?: number) => {
    const sid = sidOf(storyIdArg);
    if (!sid) { setEventLogs([]); return; }
    try {
      setEventLogs(await invoke<EventLogItem[]>("list_event_logs", { storyId: String(sid) }));
    } catch (e) {
      reportError("读取事件日志", e);
    }
  };

  const loadProposalList = async (storyIdArg?: number) => {
    const sid = sidOf(storyIdArg);
    if (!sid) { setProposals([]); return; }
    try {
      setProposals(await invoke<Proposal[]>("list_proposals", { storyId: String(sid) }));
      await loadEventLogs(sid);
    } catch (e) {
      reportError("读取演进提案", e);
    }
  };

  const loadSnapshots = async (storyIdArg?: number) => {
    const sid = sidOf(storyIdArg);
    if (!sid) { setSnapshots([]); return; }
    try {
      setSnapshots(await invoke<Snapshot[]>("list_snapshots", { storyId: String(sid) }));
    } catch (e) {
      reportError("读取设定版本", e);
    }
  };

  const loadAudit = async (storyIdArg?: number) => {
    const sid = sidOf(storyIdArg);
    if (!sid) { setAuditLogs([]); return; }
    try {
      setAuditLogs(await invoke<AuditLog[]>("list_audit_logs", { storyId: String(sid) }));
    } catch (e) {
      reportError("读取审计日志", e);
    }
  };

  return {
    proposals, setProposals,
    requireApproval, setRequireApproval, loadRequireApproval,
    snapshots, setSnapshots,
    auditLogs, setAuditLogs,
    eventLogs, setEventLogs,
    loadProposalList, loadSnapshots, loadAudit, loadEventLogs,
  };
}
