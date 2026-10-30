// 导演监视器面板（从 App.tsx 拆分，2026-10-20）。
//
// 拆分方式：**物理搬家** —— JSX 与内联样式一字未改，只把原来直接引用的
// App 局部状态改成 props（13 个）。所以这个文件的正确性可以用「行多重集相等」来证明，
// 不依赖肉眼比对。
//
// 面板内容顺序（用户指定，勿随意调整）：角色立绘 → 当前角色卡 → 关系图谱 →
// 🧩 上下文概览 → 🧾 摘要管理 → 🔍 回顾试算。

import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import type { Dispatch, SetStateAction } from "react";
import { actionOf, emotionInfo, statusText } from "../lib/chars";
import { T } from "../theme";
import type { Character, PortraitItem, Relation, StatChange, Story, TimelineNodeDto } from "../types";
import { ContextPanel } from "./ContextPanel";
import { PhaseManager } from "./PhaseManager";
import { RecallDebug } from "./RecallDebug";
import { defaultVariant, labelOfVariant, nextVariant, variantSlots } from "../lib/portraitVariants";
import type { ViewName } from "./StoryPanel";

export type DirectorPanelProps = {
  chars: Character[];
  portraits: PortraitItem[];
  relations: Relation[];
  currentStory: Story | null;
  focusCharId: number | null;
  setFocusCharId: Dispatch<SetStateAction<number | null>>;
  graphRootId: number | null;
  setGraphRootId: Dispatch<SetStateAction<number | null>>;
  setView: Dispatch<SetStateAction<ViewName>>;
  setPDetail: Dispatch<SetStateAction<number | null>>;
  ctxWindow: number;
  timelineNodes: TimelineNodeDto[];
  node: { id: number; date_label: string } | null;
  /// 撤销数值变化后要重拉角色（数值显示在 chars 里，不重拉会停在旧值）
  reloadChars: () => Promise<void>;
};

export function DirectorPanel(props: DirectorPanelProps) {
  const {
    chars, portraits, relations, currentStory, focusCharId, setFocusCharId,
    graphRootId, setGraphRootId, setView, setPDetail, ctxWindow, timelineNodes, node, reloadChars,
  } = props;
  // 「表情差分」：每个角色记住当前显示的是哪张立绘（存 **portrait id**，不是下标 ——
  // 立绘删了/新增了都不会串）。切换规则见 lib/portraitVariants.ts：
  // 按标签走、只切有图的、没有下一个就保持、一张都没有就不切。
  const [variantId, setVariantId] = useState<Record<number, number>>({});
  const cycleVariant = (chId: number, ps: PortraitItem[]) => {
    const cur = ps.find((x) => x.id === variantId[chId]) ?? defaultVariant(ps);
    const nx = nextVariant(ps, cur?.id ?? null);
    if (nx) setVariantId((v) => ({ ...v, [chId]: nx.id }));
  };
  // 三维数值变化账本（2026-10-29）：插件 stat-growth 打开时才有行；单条撤销。
  const [statChanges, setStatChanges] = useState<StatChange[]>([]);
  const [statBusyId, setStatBusyId] = useState<number | null>(null);
  const loadStatChanges = async () => {
    if (!currentStory || focusCharId == null) { setStatChanges([]); return; }
    try {
      setStatChanges(await invoke<StatChange[]>("list_stat_changes", {
        storyId: String(currentStory.id), characterId: focusCharId,
      }));
    } catch { setStatChanges([]); }
  };
  // chars 也要进依赖：日结/撤销后角色数值变了，账本列表得跟着重拉（否则停在旧变化）
  useEffect(() => { void loadStatChanges(); }, [currentStory?.id, focusCharId, chars]);
  const undoStat = async (id: number) => {
    if (!currentStory) return;
    setStatBusyId(id);
    try {
      await invoke("undo_stat_change", { storyId: String(currentStory.id), changeId: id });
      await loadStatChanges();
      await reloadChars();
    } finally {
      setStatBusyId(null);
    }
  };
  // v3.5.1：本场表演标签（最近一轮 AI 回复里的动作/情绪）—— 后台静默抽取，这里只负责显示
  const [perf, setPerf] = useState<{ actions: string[]; emotions: string[] } | null>(null);
  useEffect(() => {
    if (!currentStory || !node?.id) { setPerf(null); return; }
    void invoke<{ actions: string[]; emotions: string[] } | null>("perform_latest", {
      storyId: String(currentStory.id), nodeId: node.id,
    }).then((r) => setPerf(r)).catch(() => setPerf(null));
  }, [currentStory?.id, node?.id]);
  return (
    <div style={{ width: 300, borderLeft: `1px solid ${T.line}`, background: T.panel, flexShrink: 0, display: "flex", flexDirection: "column", minHeight: 0 }}>
      <div style={{ height: 52, flexShrink: 0, borderBottom: `1px solid ${T.line}`, display: "flex", alignItems: "center", gap: 8, padding: "0 16px" }}>
        <span style={{ color: T.accent, fontSize: 16 }}>🎬</span>
        <span style={{ fontSize: 13, fontWeight: 600 }}>导演监视器</span>
        <span style={{ marginLeft: "auto", color: T.text3, fontSize: 12, opacity: 0.45 }} title="面板设置（规划中）">⚙</span>
      </div>
      <div style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: "14px 16px 18px", display: "flex", flexDirection: "column", gap: 20 }}>
        {/* 角色立绘（占位卡片） */}
        <div>
          <div style={{ fontSize: 10.5, color: T.text3, letterSpacing: 1.1, marginBottom: 10, display: "flex", alignItems: "center", gap: 6 }}>
            <span>🖼 角色立绘</span>
            <span style={{ marginLeft: "auto" }}>{chars.length} 位角色</span>
          </div>
          <div style={{ display: "flex", gap: 10, overflowX: "auto", paddingBottom: 6 }}>
            {chars.length === 0 && (
              <div style={{ width: 86, flex: "0 0 86px", borderRadius: 12, border: `1px dashed ${T.line}`, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 4, color: T.text3, fontSize: 11, padding: "18px 0" }}>
                <span style={{ fontSize: 20 }}>🎭</span>暂无角色
              </div>
            )}
            {chars.map((ch) => {
              const emo = emotionInfo(ch.current_status || "");
              const f = emo.label ? emo : null;
              const focusNow = focusCharId ?? chars.find((x) => x.current_status)?.id ?? chars[0]?.id ?? null;
              const active = focusNow === ch.id;
              const bound = portraits.filter((p) => p.character_id === ch.id && p.story_id === currentStory?.id);
              if (bound.length === 0) {
                return (
                  <div key={ch.id} title="点击设为焦点角色（未绑定立绘，可在「立绘素材」中导入并绑定）" onClick={() => setFocusCharId(ch.id)} style={{ width: 86, flex: "0 0 86px", borderRadius: 12, overflow: "hidden", border: `1px solid ${active ? T.accent : f ? emo.color : T.line}`, background: T.panel2, cursor: "pointer", position: "relative", boxShadow: active ? `0 0 10px ${T.accent}55` : f ? `0 0 12px ${emo.color}66` : "none", transition: "box-shadow .3s" }}>
                    <div style={{ height: 104, display: "flex", alignItems: "center", justifyContent: "center", background: f ? `linear-gradient(160deg, ${emo.color}88, #22293a)` : "linear-gradient(160deg,#4a5f8a,#22293a)" }}>
                      <span style={{ fontSize: 34, color: "rgba(255,255,255,.72)" }}>🧑</span>
                    </div>
                    <div style={{ padding: "6px 7px", fontSize: 11, textAlign: "center", color: T.text2 }}>{ch.name}</div>
                    {f && <span style={{ position: "absolute", top: 5, left: 5, fontSize: 9, padding: "1px 6px", borderRadius: 4, background: emo.color, color: "#fff", fontWeight: 600 }}>{emo.label}</span>}
                    {active ? <span style={{ position: "absolute", top: 5, right: 5, fontSize: 9, padding: "1px 5px", borderRadius: 4, background: T.accent, color: "#231505", fontWeight: 700 }}>焦点</span> : <span style={{ position: "absolute", top: 5, right: 5, fontSize: 9, padding: "1px 5px", borderRadius: 4, background: "rgba(0,0,0,.55)", color: "#ffd7ad" }}>占位</span>}
                  </div>
                );
              }
              // 一个角色只出一张卡（2026-10-25 修）：原来这里是 bound.map —— 角色绑了几张立绘
              // 就渲染几张一模一样的卡，于是"同一个角色"看起来出现了好几次（用户实测反馈）。
              // 多张立绘收进同一张卡里当「表情差分」：按**标签**轮换（lib/portraitVariants.ts）。
              const slots = variantSlots(bound);
              const p = bound.find((x) => x.id === variantId[ch.id]) ?? defaultVariant(bound) ?? bound[0];
              const slotIdx = slots.findIndex((s) => s.portrait.id === p.id);
              const vlabel = labelOfVariant(p);
              return (
                <div key={ch.id}
                  title={slots.length > 1 ? `表情差分：${slots.map((s) => s.tag).join(" / ")}（点左下角换下一个）` : "点击设为焦点角色"}
                  onClick={() => setFocusCharId(ch.id)} style={{ width: 86, flex: "0 0 86px", borderRadius: 12, overflow: "hidden", border: `1px solid ${active ? T.accent : f ? emo.color : T.line}`, background: T.panel2, cursor: "pointer", position: "relative", boxShadow: active ? `0 0 10px ${T.accent}55` : f ? `0 0 12px ${emo.color}66` : "none", transition: "box-shadow .3s" }}>
                  <div style={{ height: 104, display: "flex", alignItems: "center", justifyContent: "center", background: f ? `linear-gradient(160deg, ${emo.color}88, #22293a)` : "linear-gradient(160deg,#4a5f8a,#22293a)" }}>
                    {p.path ? <img src={convertFileSrc(p.path)} alt="" style={{ width: "100%", height: "100%", objectFit: "cover", display: "block" }} /> : <span style={{ fontSize: 34, color: "rgba(255,255,255,.72)" }}>🧑</span>}
                  </div>
                  <div style={{ padding: "6px 7px", fontSize: 11, textAlign: "center", color: T.text2 }}>{ch.name}</div>
                  {f && <span style={{ position: "absolute", top: 5, left: 5, fontSize: 9, padding: "1px 6px", borderRadius: 4, background: emo.color, color: "#fff", fontWeight: 600 }}>{emo.label}</span>}
                  {p.role && <span style={{ position: "absolute", top: 5, right: 5, fontSize: 9, padding: "1px 5px", borderRadius: 4, background: "rgba(0,0,0,.55)", color: T.accent }}>{p.role}</span>}
                  {slots.length > 1 && (
                    <span
                      onClick={(e) => { e.stopPropagation(); cycleVariant(ch.id, bound); }}
                      title={`表情差分：${slots.map((s) => s.tag).join(" / ")} · 点一下换下一个（没有下一个就保持这张）`}
                      style={{ position: "absolute", left: 4, bottom: 24, maxWidth: 66, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", fontSize: 9, padding: "1px 5px", borderRadius: 4, background: "rgba(0,0,0,.6)", color: "#ffd7ad", cursor: "pointer", fontWeight: 600 }}>
                      {slotIdx + 1}/{slots.length}{vlabel ? ` ${vlabel}` : ""}
                    </span>
                  )}
                  <span onClick={(e) => { e.stopPropagation(); setView("portrait"); setPDetail(p.id); }} title="查看素材详情" style={{ position: "absolute", right: 4, bottom: 24, fontSize: 9, padding: "1px 5px", borderRadius: 4, background: "rgba(0,0,0,.6)", color: T.accent, cursor: "pointer" }}>⛶</span>
                </div>
              );
            })}
            <div title="去「立绘素材」导入并绑定角色" onClick={() => setView("portrait")} style={{ width: 86, flex: "0 0 86px", borderRadius: 12, border: `1px dashed ${T.line}`, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 4, color: T.text3, fontSize: 11, cursor: "pointer", padding: "18px 0" }}>
              <span style={{ fontSize: 20 }}>＋</span>添加
            </div>
          </div>
        </div>

        {/* 当前角色卡（焦点角色详情） */}
        <div>
          <div style={{ fontSize: 10.5, color: T.text3, letterSpacing: 1.1, marginBottom: 10, display: "flex", alignItems: "center", gap: 6 }}>
            <span>🪪 当前角色卡</span>
            <span style={{ marginLeft: "auto" }}>焦点角色 · 点立绘换表情差分</span>
          </div>
          {(() => {
            const focusCh = chars.find((c) => c.id === focusCharId) ?? chars.find((x) => x.current_status) ?? chars[0] ?? null;
            if (!focusCh) {
              return (
                <div style={{ border: `1px dashed ${T.line}`, borderRadius: 12, padding: 24, textAlign: "center", color: T.text3, fontSize: 12, background: T.panel2 }}>
                  还没有角色
                  <div style={{ marginTop: 6, fontSize: 11, color: T.text3 }}>暂无角色，先到角色管理创建</div>
                </div>
              );
            }
            const emo = emotionInfo(focusCh.current_status || "");
            // v3.5.1：动作标签（current_status 里显式写的「动作：…」）
            const act = actionOf(focusCh.current_status || "");
            // 已结构化的标签（情绪 chip / 动作行）从正文里剥掉，避免重复显示
            const rest = statusText(focusCh.current_status || "");
            const bound = portraits.filter((p) => p.character_id === focusCh.id && p.story_id === currentStory?.id);
            // 当前角色卡也跟「表情差分」联动：显示的是同一张被选中的立绘，点它换下一张
            const slots = variantSlots(bound);
            const p0 = bound.find((x) => x.id === variantId[focusCh.id]) ?? defaultVariant(bound);
            const canSwitch = slots.length > 1;
            const p0Idx = slots.findIndex((s) => s.portrait.id === p0?.id);
            return (
              <div style={{ border: `1px solid ${emo.label ? emo.color : T.line}`, borderRadius: 12, background: T.panel2, overflow: "hidden", boxShadow: emo.label ? `0 0 12px ${emo.color}44` : "none" }}>
                <div style={{ display: "flex", gap: 11, padding: 12, alignItems: "center", borderBottom: `1px solid ${T.lineSoft}` }}>
                  <div
                    onClick={() => { if (canSwitch) cycleVariant(focusCh.id, bound); }}
                    title={
                      canSwitch
                        ? `表情差分：${slots.map((s) => s.tag).join(" / ")} · 点一下换下一个`
                        : bound.length === 0 ? "这个角色还没有立绘（去「立绘素材」导入或生成）" : "只有这一张立绘"
                    }
                    style={{ width: 46, height: 46, flex: "0 0 46px", borderRadius: 12, overflow: "hidden", background: emo.label ? `linear-gradient(160deg, ${emo.color}88, #22293a)` : "linear-gradient(135deg,#7b5cff,#b45cff)", display: "flex", alignItems: "center", justifyContent: "center", fontSize: 22, position: "relative", cursor: canSwitch ? "pointer" : "default" }}>
                    {p0?.path ? <img src={convertFileSrc(p0.path)} alt="" style={{ width: "100%", height: "100%", objectFit: "cover", display: "block" }} /> : <span>🧑</span>}
                    {canSwitch && (
                      <span style={{ position: "absolute", right: 0, bottom: 0, fontSize: 8.5, padding: "0 3px", borderRadius: "6px 0 0 0", background: "rgba(0,0,0,.66)", color: "#ffd7ad", fontWeight: 700 }}>
                        {p0Idx + 1}/{slots.length}
                      </span>
                    )}
                  </div>
                  <div>
                    <div style={{ fontSize: 13.5, fontWeight: 650, display: "flex", alignItems: "center", gap: 6 }}>
                      {focusCh.name}
                      <span style={{ fontSize: 10, padding: "1px 6px", borderRadius: 5, background: T.accentSoft, color: T.accent, fontWeight: 600 }}>焦点</span>
                    </div>
                    <div style={{ fontSize: 11, color: T.text3, marginTop: 3, display: "flex", alignItems: "flex-start", gap: 6, flexWrap: "wrap", lineHeight: 1.5 }}>
                      {emo.label && <span style={{ fontSize: 10, padding: "1px 8px", borderRadius: 10, color: "#fff", background: emo.color, flex: "0 0 auto" }}>{emo.label}</span>}
                      {rest
                        ? <span>{rest}</span>
                        : (!emo.label && !act) ? <span>暂无状态</span> : null}
                    </div>
                    {/* v3.5.1：动作行 —— 紧跟在情绪/状态之后（用户要求的"动作后面"） */}
                    {act && (
                      <div style={{ fontSize: 10.5, color: T.text3, marginTop: 3 }}>
                        <span style={{ opacity: 0.7 }}>动作：</span>
                        <span style={{ color: T.text2 }}>{act}</span>
                      </div>
                    )}
                    {/* v3.5.1：本场标签 —— 从上一轮 AI 回复的（）里静默抽出来的 */}
                    {perf && (perf.actions.length > 0 || perf.emotions.length > 0) && (
                      <div style={{ fontSize: 10.5, color: T.text3, marginTop: 4, display: "flex", gap: 5, flexWrap: "wrap", alignItems: "center" }}>
                        <span style={{ opacity: 0.7 }}>本场：</span>
                        {perf.actions.map((a) => (
                          <span key={"a" + a} style={{ padding: "0 6px", borderRadius: 8, background: T.panel, border: `1px solid ${T.line}`, color: T.text2 }}>{a}</span>
                        ))}
                        {perf.emotions.map((e) => {
                          const info = emotionInfo("情绪：" + e);
                          return (
                            <span key={"e" + e} style={{ padding: "0 6px", borderRadius: 8, background: info.bg, border: `1px solid ${info.color}55`, color: info.color }}>{e}</span>
                          );
                        })}
                      </div>
                    )}
                  </div>
                </div>
                <div style={{ padding: "10px 12px 8px", fontSize: 12, color: T.text2, lineHeight: 1.5 }}>
                  <div style={{ fontSize: 10.5, color: T.text3, letterSpacing: 0.6, marginBottom: 4 }}>关系</div>
                  {(() => {
                    const rels = relations.filter((r) => r.from_character_id === focusCh.id || r.to_character_id === focusCh.id);
                    if (rels.length === 0) return <div style={{ fontSize: 11, color: T.text3 }}>暂无关系</div>;
                    const nameOf = (id: number) => chars.find((c) => c.id === id)?.name ?? `#${id}`;
                    return (
                      <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                        {rels.map((r) => {
                          const other = r.from_character_id === focusCh.id ? r.to_character_id : r.from_character_id;
                          const arrow = r.direction === "双向" ? "⟷" : r.from_character_id === focusCh.id ? "→" : "←";
                          return (
                            <div key={r.id} style={{ fontSize: 11.5, color: T.text2, display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
                              <span>{r.direction === "双向" ? `${focusCh.name} ${arrow} ${nameOf(other)}` : r.from_character_id === focusCh.id ? `${focusCh.name} ${arrow} ${nameOf(other)}` : `${nameOf(other)} ${arrow} ${focusCh.name}`}</span>
                              <span style={{ fontSize: 10, padding: "1px 7px", borderRadius: 9, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2 }}>{r.relation_type}{r.strength ? ` ${r.strength}` : ""} · {r.direction}</span>
                            </div>
                          );
                        })}
                      </div>
                    );
                  })()}
                </div>
                <div style={{ padding: "0 12px 12px", fontSize: 12, color: T.text2, lineHeight: 1.5 }}>
                  <div style={{ fontSize: 10.5, color: T.text3, letterSpacing: 0.6, marginBottom: 4 }}>角色设定</div>
                  {focusCh.profile || "（无设定）"}
                </div>
                {/* 4 项属性条：数据本来就有（characters.stat_*），角色页也在用，这里直接展示真实数值 */}
                <div style={{ padding: "11px 12px", borderTop: `1px solid ${T.lineSoft}`, display: "flex", flexDirection: "column", gap: 6 }}>
                  {([["体力", focusCh.stat_stamina, T.danger], ["理智", focusCh.stat_sanity, "#5b8cff"], ["存在感", focusCh.stat_screen, T.ok]] as [string, number, string][]).map(([lb, v, c]) => (
                    <div key={lb} style={{ display: "flex", alignItems: "center", gap: 8 }}>
                      <span style={{ fontSize: 10.5, color: T.text3, width: 28, flexShrink: 0 }}>{lb}</span>
                      <div style={{ flex: 1, height: 6, borderRadius: 3, background: "rgba(255,255,255,.07)", overflow: "hidden" }}>
                        <div style={{ width: `${Math.max(0, Math.min(100, v))}%`, height: "100%", background: c, borderRadius: 3 }} />
                      </div>
                      <span style={{ fontSize: 10, color: T.text3, width: 24, textAlign: "right", flexShrink: 0 }}>{v}</span>
                    </div>
                  ))}
                </div>
                {/* 数值变化账本（2026-10-29，插件 stat-growth）：最近 5 条 + 单条撤销。
                    放在属性条下面，是因为用户看到"体力 30"时的下一个问题就是"怎么掉成 30 的"。 */}
                {statChanges.length > 0 && (
                  <div style={{ padding: "0 12px 11px", display: "flex", flexDirection: "column", gap: 3 }}>
                    <div style={{ fontSize: 10.5, color: T.text3, letterSpacing: 0.6, marginBottom: 2 }}>数值变化（最近 5 条）</div>
                    {statChanges.slice(0, 5).map((c) => {
                      const DIM: Record<string, string> = { stamina: "体力", sanity: "理智", screen: "存在感" };
                      const undone = c.revoked === 1 || c.source === "undo";
                      return (
                        <div key={c.id} style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 10.5, color: T.text2 }}>
                          <span style={{ flexShrink: 0, width: 36, color: T.text3 }}>{DIM[c.dim] ?? c.dim}</span>
                          <span style={{ flexShrink: 0, fontWeight: 700, color: c.delta > 0 ? T.ok : T.danger }}>{c.delta > 0 ? "+" : ""}{c.delta}</span>
                          <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", color: T.text3 }} title={c.reason}>{c.kind}</span>
                          <button
                            disabled={undone || statBusyId === c.id}
                            onClick={() => void undoStat(c.id)}
                            title={undone ? "这条已经撤销" : "撤销这一条（反向补一次，历史保留）"}
                            style={{ flexShrink: 0, fontSize: 9.5, padding: "0 5px", borderRadius: 5, cursor: undone ? "default" : "pointer", border: `1px solid ${T.lineSoft}`, background: "transparent", color: undone ? T.text3 : T.text2, fontFamily: "inherit" }}
                          >撤销</button>
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>
            );
          })()}
        </div>

        {/* 关系图谱（父节点为中心 · 两层 · 单向） */}
        <div>
          <div style={{ fontSize: 10.5, color: T.text3, letterSpacing: 1.1, marginBottom: 8, display: "flex", alignItems: "center", gap: 6 }}>
            <span>🕸 关系图谱</span>
            <span style={{ marginLeft: "auto" }}>{relations.length} 条关系</span>
          </div>
          {chars.length >= 1 && (
            <div style={{ display: "flex", flexWrap: "wrap", gap: 4, marginBottom: 8 }}>
              {chars.map((c) => {
                const active = (graphRootId ?? chars.find((x) => x.current_status)?.id ?? chars[0]?.id) === c.id;
                return (
                  <span key={c.id} onClick={() => setGraphRootId(c.id)} style={{ fontSize: 10.5, padding: "2px 8px", borderRadius: 10, cursor: "pointer", border: `1px solid ${active ? T.accent : T.line}`, background: active ? T.accentSoft : "#191c22", color: active ? T.accent : T.text2, fontWeight: active ? 600 : 400 }}>
                    {c.name}
                  </span>
                );
              })}
            </div>
          )}
          {(() => {
            const rootId = graphRootId ?? chars.find((x) => x.current_status)?.id ?? chars[0]?.id ?? null;
            const root = chars.find((c) => c.id === rootId) ?? null;
            // 只取与父节点直接相关的关系（两层：中心 + 直接关联）
            const rels = relations.filter((r) => r.from_character_id === rootId || r.to_character_id === rootId);
            const nodeIds = new Set<number>();
            rels.forEach((r) => {
              if (r.from_character_id !== rootId) nodeIds.add(r.from_character_id);
              if (r.to_character_id !== rootId) nodeIds.add(r.to_character_id);
            });
            const nodes = chars.filter((c) => nodeIds.has(c.id));
            if (!root) {
              return (
                <div style={{ border: `1px dashed ${T.line}`, borderRadius: 12, padding: "32px 12px", textAlign: "center", color: T.text3, fontSize: 12, background: "#191c22" }}>暂无角色，先在角色管理里添加</div>
              );
            }
            if (rels.length === 0 || nodes.length === 0) {
              return (
                <div style={{ border: `1px dashed ${T.line}`, borderRadius: 12, padding: "32px 12px", textAlign: "center", color: T.text3, fontSize: 12, background: "#191c22" }}>
                  “{root.name}”暂无直接关系，先在关系设定里添加
                </div>
              );
            }
            const rootPos = { x: 134, y: 102 };
            const nodePos: Record<number, { x: number; y: number }> = {};
            nodes.forEach((c, i) => {
              const a = (2 * Math.PI * i) / nodes.length - Math.PI / 2;
              nodePos[c.id] = { x: 134 + 72 * Math.cos(a), y: 102 + 72 * Math.sin(a) };
            });
            return (
              <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: "#191c22", padding: 10, overflow: "hidden" }}>
                <svg width="100%" height={205} viewBox="0 0 268 205" style={{ display: "block" }}>
                  <defs>
                    <marker id="arrBack" markerWidth="6" markerHeight="6" refX="5" refY="2.5" orient="auto" markerUnits="userSpaceOnUse">
                      <path d="M0,0 L6,2.5 L0,5 Z" fill={T.text3} />
                    </marker>
                    <marker id="arrEnd" markerWidth="7" markerHeight="7" refX="6" refY="3" orient="auto" markerUnits="userSpaceOnUse">
                      <path d="M0,0 L7,3 L0,6 Z" fill={T.accent} />
                    </marker>
                  </defs>
                  {/* 关系线（from → to；双向时多一条旁边的反向虚线） */}
                  {rels.map((r) => {
                    const p1 = r.from_character_id === rootId ? rootPos : nodePos[r.from_character_id];
                    const p2 = r.to_character_id === rootId ? rootPos : nodePos[r.to_character_id];
                    if (!p1 || !p2) return null;
                    const dx = p2.x - p1.x, dy = p2.y - p1.y;
                    const len = Math.sqrt(dx * dx + dy * dy) || 1;
                    const r1 = r.from_character_id === rootId ? 20 : 18;
                    const r2 = r.to_character_id === rootId ? 20 : 18;
                    const sx = p1.x + (dx / len) * r1;
                    const sy = p1.y + (dy / len) * r1;
                    const ex = p2.x - (dx / len) * r2;
                    const ey = p2.y - (dy / len) * r2;
                    return (
                      <g key={r.id}>
                        {/* 双向关系：在主线旁边画一条**反向虚线**（2026-10-30 用户要求，不能与主线重合）。
                            偏移取主线的法向 4px，两端各自留出节点半径。 */}
                        {r.direction === "双向" && (() => {
                          const nx = -dy / len, ny = dx / len; // 法向单位向量
                          const off = 4;
                          const bx1 = sx + nx * off, by1 = sy + ny * off;
                          const bx2 = ex + nx * off, by2 = ey + ny * off;
                          return (
                            <line x1={bx2} y1={by2} x2={bx1} y2={by1}
                              stroke={T.text3} strokeWidth={1.4} strokeDasharray="4 3" markerEnd="url(#arrBack)" opacity={0.85} />
                          );
                        })()}
                        <line x1={sx} y1={sy} x2={ex} y2={ey} stroke={T.accent} strokeWidth={2} markerEnd="url(#arrEnd)" />
                        <text x={(sx + ex) / 2} y={(sy + ey) / 2 - 5} textAnchor="middle" fontSize="10" fill="#fff" fontWeight={600} paintOrder="stroke" stroke="#191c22" strokeWidth={3} style={{ userSelect: "none" }}>
                          {r.relation_type}{r.strength ? ` ${r.strength}` : ""}
                        </text>
                      </g>
                    );
                  })}
                  {/* 中心父节点 */}
                  <circle cx={rootPos.x} cy={rootPos.y} r={20} fill={T.accentSoft} stroke={T.accent} strokeWidth={1.6} />
                  <text x={rootPos.x} y={rootPos.y + 3.5} textAnchor="middle" fontSize="10.5" fill={T.accent} fontWeight={700} style={{ userSelect: "none" }}>{root.name}</text>
                  {/* 一层关联角色 */}
                  {nodes.map((c) => (
                    <g key={c.id}>
                      <circle cx={nodePos[c.id].x} cy={nodePos[c.id].y} r={18} fill={T.panel3} stroke={T.line} strokeWidth={1.2} />
                      <text x={nodePos[c.id].x} y={nodePos[c.id].y + 3.5} textAnchor="middle" fontSize="10" fill={T.text1} fontWeight={600} style={{ userSelect: "none" }}>{c.name}</text>
                    </g>
                  ))}
                </svg>
              </div>
            );
          })()}
        </div>
        {/* 🧩 上下文概览（A2）：一眼看出"这次装配对不对"（按用户要求放在关系图谱下面）*/}
        <ContextPanel storyId={currentStory ? String(currentStory.id) : null} ctxWindow={ctxWindow} />
        <div style={{ height: 1, background: T.line, margin: "14px 0" }} />
        {/* 🧾 摘要管理（⑥ 阶段摘要）：看/删/压 */}
        <PhaseManager
        storyId={currentStory ? String(currentStory.id) : null}
        summarizedDays={timelineNodes.filter((n) => (n.node_type === "day" || n.node_type === "main") && (n.day_summary ?? "").trim() !== "").length}
        totalDays={timelineNodes.filter((n) => n.node_type === "day" || n.node_type === "main").length}
        />
        {/* 🔍 回顾试算（C3）：输入一句话，看会带哪几天、为什么、各得多少分 */}
        <RecallDebug storyId={currentStory ? String(currentStory.id) : null} nodeId={node?.id ?? null} />
        <div style={{ height: 1, background: T.line, margin: "14px 0" }} />
      </div>
    </div>
  );
}
