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

  const loadRequireApproval = async () => {
    try {
      setRequireApproval(await invoke<boolean>("get_evolution_require_approval"));
    } catch (e) {
      // 读不到审批开关时保持安全默认值 true；这是明确的降级，不需要打扰用户。
      expectedError("读取演进审批开关失败，按默认 true 降级", e);
    }
  };

  const loadEventLogs = async () => {
    try {
      setEventLogs(await invoke<EventLogItem[]>("list_event_logs", { storyId: String(currentStory?.id) }));
    } catch (e) {
      reportError("读取事件日志", e);
    }
  };

  const loadProposalList = async () => {
    try {
      setProposals(await invoke<Proposal[]>("list_proposals", { storyId: String(currentStory?.id) }));
      await loadEventLogs();
    } catch (e) {
      reportError("读取演进提案", e);
    }
  };

  const loadSnapshots = async () => {
    try {
      setSnapshots(await invoke<Snapshot[]>("list_snapshots", { storyId: String(currentStory?.id) }));
    } catch (e) {
      reportError("读取设定版本", e);
    }
  };

  const loadAudit = async () => {
    try {
      setAuditLogs(await invoke<AuditLog[]>("list_audit_logs", { storyId: String(currentStory?.id) }));
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
