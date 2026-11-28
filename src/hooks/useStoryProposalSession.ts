import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AuditLog, EventLogItem, Proposal, Snapshot, Story } from "../types";

export function useStoryProposalSession(currentStory: Story | null) {
  const [proposals, setProposals] = useState<Proposal[]>([]);
  const [requireApproval, setRequireApproval] = useState(true);
  const [snapshots, setSnapshots] = useState<Snapshot[]>([]);
  const [auditLogs, setAuditLogs] = useState<AuditLog[]>([]);
  const [eventLogs, setEventLogs] = useState<EventLogItem[]>([]);

  const loadRequireApproval = async () => {
    try { setRequireApproval(await invoke<boolean>("get_evolution_require_approval")); } catch { /* 读不到按默认 true */ }
  };

  const loadEventLogs = async () => {
    try { setEventLogs(await invoke<EventLogItem[]>("list_event_logs", { storyId: String(currentStory?.id) })); } catch (e) { console.error(e); }
  };

  const loadProposalList = async () => {
    try {
      setProposals(await invoke<Proposal[]>("list_proposals", { storyId: String(currentStory?.id) }));
      await loadEventLogs();
    } catch (e) { console.error(e); }
  };

  const loadSnapshots = async () => {
    try { setSnapshots(await invoke<Snapshot[]>("list_snapshots", { storyId: String(currentStory?.id) })); } catch (e) { console.error(e); }
  };

  const loadAudit = async () => {
    try { setAuditLogs(await invoke<AuditLog[]>("list_audit_logs", { storyId: String(currentStory?.id) })); } catch (e) { console.error(e); }
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
