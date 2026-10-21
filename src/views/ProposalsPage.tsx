// 「ProposalsPage」页（从 App.tsx 拆分，2026-10-17）。
//
// 拆分方式：视图 + 本页专属的 state/处理函数一起下沉（见 ROADMAP §10.66）。
// 只保留跨页共享项作为 props。

import { useState, type Dispatch, type SetStateAction } from "react";
import type { AppView } from "../types";
import { eventLabel } from "../lib/parse";
import { T } from "../theme";
import type { AuditLog, EventLogItem, Proposal, SnapDiff, Snapshot, Story, TimelineNodeDto, Character, Relation } from "../types";
import { charNameOf } from "../lib/chars";
import { invoke } from "@tauri-apps/api/core";

export type ViewName = AppView;

export type ProposalsPageProps = {
  setSnapPreview: Dispatch<SetStateAction<{ version: number; diff: SnapDiff;
    loading: boolean; error: string } | null>>;
  analyzeInfo: { text: string; bad: boolean } | null;
  auditLogs: AuditLog[];
  chars: Character[];
  currentStory: Story | null;
  eventLogs: EventLogItem[];
  loadAudit: () => Promise<void>;
  loadChars: (storyId: number) => Promise<void>;
  loadProposals: () => Promise<void>;
  loadRelations: (storyId: number) => Promise<void>;
  loadSnapshots: () => Promise<void>;
  node: { id: number; date_label: string } | null;
  proposals: Proposal[];
  relations: Relation[];
  requireApproval: boolean;
  selGroups: Set<number>;
  setAnalyzeInfo: Dispatch<SetStateAction<{ text: string; bad: boolean } | null>>;
  setMsg: Dispatch<SetStateAction<string>>;
  setRequireApproval: Dispatch<SetStateAction<boolean>>;
  setSelGroups: Dispatch<SetStateAction<Set<number>>>;
  snapshots: Snapshot[];
  timelineNodes: TimelineNodeDto[];
};

export function ProposalsPage(props: ProposalsPageProps) {
  const { setSnapPreview, analyzeInfo, auditLogs, chars, currentStory, eventLogs, loadAudit, loadChars, loadProposals, loadRelations, loadSnapshots, node, proposals, relations, requireApproval, selGroups, setAnalyzeInfo, setMsg, setRequireApproval, setSelGroups, snapshots, timelineNodes } = props;
  // ===== 从 App.tsx 搬进来的本页状态与处理函数 =====
  const [showHandled, setShowHandled] = useState(false);
  const toggleRequireApproval = async (on: boolean) => {
    try {
      await invoke("set_evolution_require_approval", { on });
      setRequireApproval(on);
      setMsg(on ? "🔒 已开启：设定变更一律进入待确认队列" : "⚠ 已关闭：低风险变更（关系强度）会立即自动应用");
    } catch (e) { setMsg("❌ " + String(e)); }
  };
  const analyzeCurrent = async () => {
    if (!node) {
      const m = "⚠️ 请先在「时间线」里点一个剧情节点，再回来分析（分析是针对「当前节点」的）";
      setMsg(m);
      setAnalyzeInfo({ text: m, bad: true });
      return;
    }
    setAnalyzeInfo({ text: `⏳ 正在分析「${node.date_label}」…`, bad: false });
    try {
      const rep = await invoke<{ events: { event_type: string; matched: string; confidence: number }[]; proposals: Proposal[]; applied_groups: number[]; notes: string[] }>(
        "analyze_evolution", { storyId: String(currentStory?.id), nodeId: node.id }
      );
      const evs = rep.events.map((e) => e.event_type).join(", ");
      // ⑤-B：语义变了 —— 现在默认"只入队"，所以要把"待确认"和"已自动应用"分开说，
      // 否则用户会以为提案已经改库了。
      const queued = rep.proposals.filter((x) => x.status === "pending").length;
      const info =
        `「${node.date_label}」分析完成 · 事件[${evs || "无"}] · `
        + (queued > 0 ? `新增 ${queued} 条待确认` : "无新增提案")
        + (rep.applied_groups.length > 0 ? `，自动应用 ${rep.applied_groups.length} 组` : "，未改库（等你拍板）")
        + (rep.notes.length ? " · " + rep.notes.join("；") : "");
      setMsg("✅ " + info);
      setAnalyzeInfo({ text: "✅ " + info, bad: false });
      await loadProposals();
      await loadRelations(currentStory!.id);
      await loadChars(currentStory!.id);
    } catch (e) {
      const m = "❌ 分析失败：" + String(e);
      setMsg(m);
      setAnalyzeInfo({ text: m, bad: true });
    }
  };
  const injectProposal = async () => {
    try {
      const m = await invoke<string>("selftest_inject_proposal", {
        storyId: String(currentStory?.id),
        nodeId: node?.id ?? null,
      });
      setAnalyzeInfo({ text: "🧪 " + m, bad: false });
      setMsg("🧪 " + m);
      await loadProposals();
    } catch (e) {
      const m = "❌ 注入失败：" + String(e);
      setAnalyzeInfo({ text: m, bad: true });
      setMsg(m);
    }
  };
  const confirmSelected = async () => {
    if (selGroups.size === 0) return;
    try {
      setMsg("⏳ " + await invoke<string>("confirm_proposals_batch", {
        storyId: String(currentStory?.id),
        groupIds: Array.from(selGroups),
      }));
      setSelGroups(new Set());
      await loadProposals();
      await loadSnapshots(); await loadAudit();
      if (currentStory) await loadRelations(currentStory.id);
    } catch (e) { setMsg("❌ " + String(e)); }
  };
  const rejectSelected = async () => {
    if (selGroups.size === 0) return;
    try {
      setMsg("🗑 " + await invoke<string>("reject_proposals_batch", {
        storyId: String(currentStory?.id),
        groupIds: Array.from(selGroups),
      }));
      setSelGroups(new Set());
      await loadProposals();
    } catch (e) { setMsg("❌ " + String(e)); }
  };
  const confirmGroup = async (gid: number) => {
    try {
      const r = await invoke<string>("confirm_proposal_group", { storyId: String(currentStory?.id), groupId: gid });
      setMsg("✅ " + r);
      await loadProposals();
      await loadRelations(currentStory!.id);
    } catch (e) { setMsg("❌ " + String(e)); }
  };
  const rejectGroup = async (gid: number) => {
    try {
      const r = await invoke<string>("reject_proposal_group", { storyId: String(currentStory?.id), groupId: gid });
      setMsg("🟡 " + r);
      await loadProposals();
    } catch (e) { setMsg("❌ " + String(e)); }
  };
  const keepSnapshot = async (version: number) => {
    if (!currentStory) return;
    try {
      setMsg("💾 " + await invoke<string>("keep_snapshot", { storyId: String(currentStory.id), version }));
      await loadSnapshots();
    } catch (e) { setMsg("❌ " + String(e)); }
  };
  const purgeTempSnapshots = async () => {
    if (!currentStory) return;
    try {
      const n = await invoke<number>("purge_temporary_snapshots", { storyId: String(currentStory.id) });
      setMsg(n > 0 ? `🧹 已清理 ${n} 个临时设定版本` : "没有需要清理的临时设定版本");
      await loadSnapshots();
    } catch (e) { setMsg("❌ 清理失败: " + String(e)); }
  };
  const openRollbackPreview = async (v: number) => {
    if (!currentStory) return;
    setSnapPreview({ version: v, diff: { characters: [], relations: [], world_settings: [] }, loading: true, error: "" });
    try {
      const diff = await invoke<SnapDiff>("preview_snapshot", { storyId: String(currentStory.id), version: v });
      setSnapPreview({ version: v, diff, loading: false, error: "" });
    } catch (e) {
      setSnapPreview({ version: v, diff: { characters: [], relations: [], world_settings: [] }, loading: false, error: String(e) });
    }
  };
  const proposalRelationLabel = (p: Proposal) => {
    const rel = relations.find((r) => r.id === p.target_id);
    if (!rel) return `关系 #${p.target_id}`;
    return `${charNameOf(chars, rel.from_character_id)} ${rel.direction === "双向" ? "⟷" : "→"} ${charNameOf(chars, rel.to_character_id)}`;
  };
  return (
      (
        <div>
          <div style={{ display: "flex", gap: 8, alignItems: "center", marginBottom: 8, flexWrap: "wrap" }}>
            <h3 style={{ margin: 0 }}>⏳ 待确认设定</h3>
            <button onClick={() => void analyzeCurrent()}>分析当前节点{node ? `（${node.date_label}）` : ""}</button>
            <button onClick={() => void loadProposals()}>刷新</button>
            <button onClick={() => void injectProposal()} title="不依赖 AI 剧情信号，直接造一条待确认提案 —— 用来稳定验证「入队 → 勾选 → 落地 → 回滚」"
              style={{ fontSize: 11.5, padding: "3px 10px", borderRadius: 6, border: `1px dashed ${T.line}`, background: "transparent", color: T.text3, cursor: "pointer", fontFamily: "inherit" }}>🧪 注入测试提案</button>
            <label style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 7, fontSize: 12, color: T.text2, cursor: "pointer" }}>
              <input type="checkbox" checked={requireApproval} onChange={(e) => void toggleRequireApproval(e.target.checked)} style={{ cursor: "pointer" }} />
              设定变更必须先拍板
            </label>
          </div>
          {/* 就地状态行：分析结果 / 错误都显示在这里，不依赖页面顶部的全局 msg */}
          {analyzeInfo && (
            <div style={{ marginBottom: 10, padding: "7px 11px", borderRadius: 8, fontSize: 12, lineHeight: 1.6,
              border: `1px solid ${analyzeInfo.bad ? T.danger + "66" : T.line}`,
              background: analyzeInfo.bad ? "rgba(240,85,77,.10)" : T.panel2,
              color: analyzeInfo.bad ? T.danger : T.text2 }}>
              {analyzeInfo.text}
            </div>
          )}
          {!node && (
            <div style={{ marginBottom: 10, fontSize: 11.5, color: T.warn }}>
              ⚠ 还没有选中剧情节点 ——「分析当前节点」需要先在「时间线」里点一天。若只是想验证拍板流程，可直接点上面的「🧪 注入测试提案」。
            </div>
          )}
          <div style={{ fontSize: 11.5, color: T.text3, lineHeight: 1.8, marginBottom: 12 }}>
            {requireApproval ? (
              <>AI 写完后会自动抽取"设定层面的变化"（目前是**关系推进**：类型 / 方向 / 强度），
                只**累积到这个队列**，点「落地」才会真正改库 —— 每次落地都会留一个可回滚的设定版本。</>
            ) : (
              <span style={{ color: T.warn }}>⚠ 开关已关闭：低风险的"关系强度"变更会**立即自动应用**（不经你确认）。涉及关系类型 / 方向的仍会入队。</span>
            )}
            <br />
            <span style={{ color: T.text3 }}>（注意：你在关系图谱里手改不会生成提案 —— 那是直接改库；只有引擎根据剧情推导出的提案才会出现在这里。想基于剧情推导，请点「分析当前节点」而不是「刷新」）</span>
          </div>
          {(() => {
            const TYPE_LABEL: Record<string, string> = { relation: "🤝 关系", character: "👤 角色", world: "🌍 世界观" };
            const pendingGroups = Array.from(new Set(proposals.filter((p) => p.status === "pending").map((p) => p.group_id)));
            const handledGroups = Array.from(new Set(proposals.filter((p) => p.status !== "pending").map((p) => p.group_id)));
            const byType = (gids: number[]) => {
              const m = new Map<string, number[]>();
              for (const gid of gids) {
                const g = proposals.filter((p) => p.group_id === gid);
                const tt = g[0]?.target_type ?? "relation";
                if (!m.has(tt)) m.set(tt, []);
                m.get(tt)!.push(gid);
              }
              return Array.from(m.entries());
            };
            const allSel = pendingGroups.length > 0 && pendingGroups.every((g) => selGroups.has(g));
            const toggleAll = () => setSelGroups(allSel ? new Set() : new Set(pendingGroups));
            const toggleOne = (gid: number) => {
              const s = new Set(selGroups);
              if (s.has(gid)) s.delete(gid); else s.add(gid);
              setSelGroups(s);
            };
            const stBadge = (st: string) =>
              st === "pending" ? { text: "⏳ 待确认", color: T.warn, bg: "rgba(245,181,68,.12)" }
              : st === "applied" ? { text: "✅ 已落地", color: T.ok, bg: "rgba(55,201,120,.12)" }
              : { text: "⛔ 已拒绝", color: T.danger, bg: "rgba(240,85,77,.12)" };

            const card = (gid: number) => {
              const group = proposals.filter((p) => p.group_id === gid);
              const st = group[0].status;
              const badge = stBadge(st);
              const nodeId = group[0].node_id;
              const srcLabel = nodeId != null
                ? (timelineNodes.find((n2) => n2.id === nodeId)?.date_label ?? `节点 #${nodeId}`)
                : null;
              return (
                <div key={gid} style={{ border: `1px solid ${st === "pending" ? T.line : T.lineSoft}`, padding: 11, borderRadius: 8, marginBottom: 8, background: st === "pending" ? T.panel2 : "transparent" }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 6 }}>
                    {st === "pending" && (
                      <input type="checkbox" checked={selGroups.has(gid)} onChange={() => toggleOne(gid)} style={{ cursor: "pointer" }} />
                    )}
                    <span style={{ fontWeight: 600, fontSize: 12.5 }}>组 #{gid} · {proposalRelationLabel(group[0])}</span>
                    {srcLabel && <span style={{ fontSize: 11, color: T.text3 }}>来源：{srcLabel}</span>}
                    <span style={{ marginLeft: "auto", fontSize: 11, padding: "2px 8px", borderRadius: 10, color: badge.color, background: badge.bg }}>{badge.text}</span>
                  </div>
                  <div style={{ fontSize: 12.5, color: T.text1, marginBottom: 4 }}>
                    {group.map((p) => (
                      <div key={p.id}>
                        {p.field === "strength" ? "强度" : p.field === "direction" ? "方向" : p.field === "relation_type" ? "关系类型" : p.field}：
                        <b>{p.old_value}</b> → <b style={{ color: T.accent }}>{p.new_value}</b>
                        <span style={{ color: T.text3, marginLeft: 6 }}>（置信度 {p.confidence.toFixed(2)}）</span>
                      </div>
                    ))}
                  </div>
                  <div style={{ fontSize: 11.5, color: T.text3 }}>原因：{group[0].reason}</div>
                  <div style={{ fontSize: 11.5, color: T.text3, marginTop: 2 }}>证据：{group[0].evidence}</div>
                  {group[0].note && <div style={{ fontSize: 11.5, color: T.danger, marginTop: 2 }}>备注：{group[0].note}</div>}
                  {st === "pending" && (
                    <div style={{ display: "flex", gap: 8, marginTop: 8 }}>
                      <button onClick={() => void confirmGroup(gid)} style={{ background: T.ok, color: "#0c2318", border: "none", padding: "5px 12px", borderRadius: 6, fontSize: 12, fontWeight: 600, cursor: "pointer", fontFamily: "inherit" }}>落地</button>
                      <button onClick={() => void rejectGroup(gid)} style={{ background: "transparent", color: T.danger, border: `1px solid ${T.danger}55`, padding: "5px 12px", borderRadius: 6, fontSize: 12, cursor: "pointer", fontFamily: "inherit" }}>拒绝</button>
                    </div>
                  )}
                </div>
              );
            };

            if (proposals.length === 0) {
              return <p style={{ color: T.text3, fontSize: 13 }}>暂无提案。先写剧情生成回复，再点「分析当前节点」。</p>;
            }

            return (
              <>
                {/* ── 待确认 ── */}
                <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 8 }}>
                  <span style={{ fontSize: 13, fontWeight: 650 }}>待确认（{pendingGroups.length} 组 / {pendingGroups.length ? proposals.filter((p) => p.status === "pending").length : 0} 条）</span>
                  {pendingGroups.length > 0 && (
                    <>
                      <button onClick={toggleAll} style={{ fontSize: 11.5, padding: "3px 10px", borderRadius: 6, border: `1px solid ${T.line}`, background: "transparent", color: T.text2, cursor: "pointer", fontFamily: "inherit" }}>{allSel ? "取消全选" : "全选"}</button>
                      <button onClick={() => void confirmSelected()} disabled={selGroups.size === 0}
                        style={{ fontSize: 11.5, padding: "3px 12px", borderRadius: 6, border: "none", background: selGroups.size ? T.ok : T.line, color: selGroups.size ? "#0c2318" : T.text3, fontWeight: 600, cursor: selGroups.size ? "pointer" : "default", fontFamily: "inherit" }}>
                        ✅ 落地选中（{selGroups.size}）
                      </button>
                      <button onClick={() => void rejectSelected()} disabled={selGroups.size === 0}
                        style={{ fontSize: 11.5, padding: "3px 12px", borderRadius: 6, border: `1px solid ${selGroups.size ? T.danger + "55" : T.line}`, background: "transparent", color: selGroups.size ? T.danger : T.text3, cursor: selGroups.size ? "pointer" : "default", fontFamily: "inherit" }}>
                        拒绝选中
                      </button>
                    </>
                  )}
                </div>
                {pendingGroups.length === 0 ? (
                  <p style={{ color: T.text3, fontSize: 12.5, marginBottom: 12 }}>队列是空的 ✅ 分析剧情后如果有设定变化会自动出现在这里。</p>
                ) : (
                  <div style={{ marginBottom: 14 }}>
                    {byType(pendingGroups).map(([tt, gids]) => (
                      <div key={tt} style={{ marginBottom: 10 }}>
                        <div style={{ fontSize: 11.5, color: T.text3, letterSpacing: 0.6, margin: "8px 0 6px" }}>{TYPE_LABEL[tt] ?? tt}（{gids.length}）</div>
                        {gids.map((g) => card(g))}
                      </div>
                    ))}
                  </div>
                )}

                {/* ── 已处理 ── */}
                {handledGroups.length > 0 && (
                  <div style={{ marginBottom: 14 }}>
                    <button onClick={() => setShowHandled(!showHandled)}
                      style={{ background: "transparent", border: "none", color: T.text3, fontSize: 11.5, cursor: "pointer", fontFamily: "inherit", padding: 0 }}>
                      {showHandled ? "▾" : "▸"} 已处理（{handledGroups.length} 组）
                    </button>
                    {showHandled && (
                      <div style={{ marginTop: 8 }}>
                        {byType(handledGroups).map(([tt, gids]) => (
                          <div key={tt} style={{ marginBottom: 8 }}>
                            <div style={{ fontSize: 11.5, color: T.text3, margin: "6px 0 4px" }}>{TYPE_LABEL[tt] ?? tt}</div>
                            {gids.map((g) => card(g))}
                          </div>
                        ))}
                      </div>
                    )}
                  </div>
                )}
              </>
            );
          })()}

          <hr style={{ margin: "20px 0" }} />

          {/* 设定版本（原叫"快照"，与设置页的"请求记录"撞名，已改名） */}
          <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 4 }}>
            <h3 style={{ margin: 0 }}>🗂 设定版本（可回滚）</h3>
            <button onClick={() => void purgeTempSnapshots()} title="清掉回滚前自动生成的临时版本（程序启动时也会自动清）"
              style={{ fontSize: 11.5, padding: "2px 10px", borderRadius: 6, cursor: "pointer", fontFamily: "inherit" }}>🧹 清理临时版本</button>
          </div>
          <div style={{ fontSize: 11.5, color: T.text3, marginBottom: 10, lineHeight: 1.7 }}>
            每个版本记录"当时"的角色 / 关系 / 世界观。点「回滚到此版本」会**先显示差异**（会改什么、会删什么），确认后才替换。
            <br />
            <span style={{ color: T.warn }}>注意：只有"应用提案"时才会自动存版本；直接在角色管理 / 世界观里手改不会存 —— 重要改动前建议先应用一次提案。</span>
            <br />
            带 <span style={{ color: T.warn }}>⏳ 临时</span> 的是**自动存的备份**（回滚前、以及**首次直接编辑设定前**各存一次）：它只在本次运行期间有效，退出程序会自动清除 —— 所以改错了 / 回滚错了，可以用它退回来（但在关掉软件之前要退）。
            <br />
            想把某个临时版本**留住**？点它右边的「<span style={{ color: T.ok }}>保存为永久</span>」，它就会变成普通版本，不再被自动清理。
          </div>
          {snapshots.length === 0 ? (
            <p style={{ color: "#6f7686", fontSize: 13 }}>暂无设定版本。每次应用提案后会自动生成一个新的设定版本。</p>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 6, marginBottom: 12 }}>
              {snapshots.map((s) => (
                <div key={s.id} style={{ border: `1px solid ${s.temporary ? T.warn : "#2e333d"}`, padding: "8px 12px", borderRadius: 8, display: "flex", justifyContent: "space-between", alignItems: "center", gap: 10 }}>
                  <div style={{ fontSize: 13, minWidth: 0 }}>
                    <span style={{ fontWeight: s.temporary ? 650 : 400 }}>V{s.version}</span>
                    {s.temporary ? (
                      <span title="回滚前的自动备份：只在本次运行期间有效，退出程序会自动清除"
                        style={{ marginLeft: 8, fontSize: 10, padding: "1px 7px", borderRadius: 8, border: `1px solid ${T.warn}`, color: T.warn }}>⏳ 临时</span>
                    ) : null}
                    <span style={{ color: T.text3 }}> · {s.created_at} {s.parent_version != null ? `（父 V${s.parent_version}）` : "（初始）"}</span>
                    {s.note ? <div style={{ fontSize: 11, color: T.text3, marginTop: 2 }}>{s.note}</div> : null}
                  </div>
                  <div style={{ display: "flex", gap: 6, flexShrink: 0 }}>
                    {s.temporary ? (
                      <button onClick={() => void keepSnapshot(s.version)}
                        title="把这条临时版本变成永久版本（不再被自动清理）"
                        style={{ fontSize: 12, color: "#7ddf9c", border: "1px solid rgba(55,201,120,.45)", background: "#22262f", padding: "3px 10px", borderRadius: 6, cursor: "pointer", fontFamily: "inherit" }}>保存为永久</button>
                    ) : null}
                    <button onClick={() => void openRollbackPreview(s.version)} style={{ fontSize: 12, color: "#ff8c85", border: "1px solid rgba(240,85,77,.4)", background: "#22262f", padding: "3px 10px", borderRadius: 6, cursor: "pointer", fontFamily: "inherit" }}>回滚到此版本</button>
                  </div>
                </div>
              ))}
            </div>
          )}

          <h3 style={{ margin: 0, marginBottom: 8 }}>📜 审计日志</h3>
          {auditLogs.length === 0 ? (
            <p style={{ color: "#6f7686", fontSize: 13 }}>暂无审计记录。</p>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 4, marginBottom: 12 }}>
              {auditLogs.map((a) => (
                <div key={a.id} style={{ fontSize: 12, color: "#a3a9b8" }}>
                  [{a.created_at}] {a.action} · {a.target || "-"} · {a.operator}
                </div>
              ))}
            </div>
          )}

          <h3 style={{ margin: 0, marginBottom: 8 }}>📌 事件日志（分析器结果）</h3>
          {eventLogs.length === 0 ? (
            <p style={{ color: "#6f7686", fontSize: 13 }}>暂无事件日志。</p>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
              {eventLogs.map((e) => (
                <div key={e.id} style={{ fontSize: 12, color: "#a3a9b8" }}>
                  #{e.id} node{e.node_id ?? "-"} · <b style={{ color: T.text2 }}>{eventLabel(e.event_type)}</b> · 置信度 {e.confidence.toFixed(2)} · {e.source}
                </div>
              ))}
            </div>
          )}
        </div>
      )
  );
}
