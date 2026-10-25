// 「WorldPage」页（从 App.tsx 拆分，2026-10-17）。
//
// 拆分方式：视图 + 本页专属的 state/处理函数一起下沉（见 ROADMAP §10.66）。
// 只保留跨页共享项作为 props。

import { useState, type Dispatch, type SetStateAction } from "react";
import type { AppView } from "../types";
import { T } from "../theme";
import type { WorldConflict, WorldSetting, Story } from "../types";
import { invoke } from "@tauri-apps/api/core";
import { useConfirm } from "../confirm";

export type ViewName = AppView;

export type WorldPageProps = {
  currentStory: Story | null;
  ensureEditBackup: (what: string) => Promise<void>;
  loadWorlds: (storyId: number) => Promise<void>;
  runCheck: (board: "characters" | "world") => Promise<void>;
  setMsg: Dispatch<SetStateAction<string>>;
  worlds: WorldSetting[];
};

export function WorldPage(props: WorldPageProps) {
  const confirm = useConfirm();
  const { currentStory, ensureEditBackup, loadWorlds, runCheck, setMsg, worlds } = props;
  // ===== 从 App.tsx 搬进来的本页状态与处理函数 =====
  const [worldTab, setWorldTab] = useState<"entries" | "map" | "factions" | "terms" | "items" | "lore" | "conflicts">("map");
  const [worldLoading, setWorldLoading] = useState(false);
  const [worldConflicts, setWorldConflicts] = useState<WorldConflict[]>([]);
  const [worldSelId, setWorldSelId] = useState<number | null>(null);
  const [worldSearch, setWorldSearch] = useState("");
  const [wEditId, setWEditId] = useState<number | null>(null);
  const [wDraft, setWDraft] = useState<{ key: string; value: string; category: string; hard: boolean; meta: Record<string, string> }>({ key: "", value: "", category: "设定", hard: false, meta: {} });
  const [facRelEditId, setFacRelEditId] = useState<number | null>(null);
  const [facRelDraft, setFacRelDraft] = useState({ from: "", to: "", relation: "中立" });
  const removeWorld = async (id: number, key: string) => {
    if (!currentStory) return;
    if (!(await confirm({ title: "删除世界观条目", body: `删除世界观条目「${key}」？`, danger: true }))) return;
    try {
      await invoke("delete_world_setting", { storyId: String(currentStory.id), id });
      setMsg(`🗑️ 已删除「${key}」`);
      await loadWorlds(currentStory.id);
    } catch (e) {
      setMsg("❌ 删除失败: " + String(e));
    }
  };
  return (
      (() => {
        const catOf = (c: string) => worlds.filter((w) => w.category === c);
        const metaOf = (w: WorldSetting) => { try { return JSON.parse(w.meta_json || "{}") as Record<string, string>; } catch { return {} as Record<string, string>; } };
        const locs = catOf("地理");
        const facs = catOf("势力");
        const facRels = catOf("势力关系");
        const terms = catOf("术语");
        // v3.4：物品库（与术语同构：名称 + 说明 + 别名）
        const items = catOf("物品");
        // v3.4：世界书（触发式注入）
        const lores = catOf("世界书");
        const plain = worlds.filter((w) => !["地理", "势力", "势力关系", "术语", "物品", "世界书"].includes(w.category));
        const hardCount = worlds.filter((w) => w.is_hard_rule === 1).length;
        const resetDraft = (category: string, meta: Record<string, string> = {}) => { setWEditId(null); setWDraft({ key: "", value: "", category, hard: false, meta }); };
        const saveDraft = async () => {
          if (!currentStory) return;
          if (!wDraft.key.trim() || !wDraft.value.trim()) { setMsg("❌ 请填写名称和内容"); return; }
          const metaJson = JSON.stringify(wDraft.meta || {});
          try {
            if (wEditId) {
              await ensureEditBackup("修改世界观设定");
    await invoke("update_world_setting", { storyId: String(currentStory.id), id: wEditId, key: wDraft.key.trim(), value: wDraft.value.trim(), isHardRule: wDraft.hard, category: wDraft.category, metaJson });
              setMsg("✅ 设定已更新");
            } else {
              await ensureEditBackup("新建世界观设定");
    await invoke("create_world_setting", { storyId: String(currentStory.id), key: wDraft.key.trim(), value: wDraft.value.trim(), isHardRule: wDraft.hard, category: wDraft.category, metaJson });
              setMsg("✅ 设定已添加");
            }
            setWEditId(null);
            setWDraft({ key: "", value: "", category: wDraft.category, hard: false, meta: {} });
            await loadWorlds(currentStory.id);
          } catch (e) { setMsg("❌ 保存失败: " + String(e)); }
        };
        const editDraft = (w: WorldSetting) => { setWEditId(w.id); setWDraft({ key: w.key, value: w.value, category: w.category, hard: w.is_hard_rule === 1, meta: metaOf(w) }); };
        const detect = async () => {
          if (!currentStory) return;
          setWorldLoading(true);
          try { setWorldConflicts(await invoke<WorldConflict[]>("detect_world_conflicts", { storyId: String(currentStory.id) })); }
          catch (e) { setMsg("❌ 冲突检测失败: " + String(e)); }
          finally { setWorldLoading(false); }
        };
        const dropEntry = async (id: number, label: string) => {
          if (!currentStory) return;
          if (!(await confirm({ title: "删除设定", body: `删除设定「${label}」？`, danger: true }))) return;
          try { await ensureEditBackup("删除世界观设定"); await invoke("delete_world_setting", { storyId: String(currentStory.id), id }); setMsg("🗑️ 已删除"); await loadWorlds(currentStory.id); await detect(); }
          catch (e) { setMsg("❌ 删除失败: " + String(e)); }
        };
        const saveFacRel = async () => {
          if (!currentStory || !facRelDraft.from || !facRelDraft.to) { setMsg("❌ 请选择两个势力"); return; }
          if (facRelDraft.from === facRelDraft.to) { setMsg("❌ 两个势力不能相同"); return; }
          const metaJson = JSON.stringify({ from: facRelDraft.from, to: facRelDraft.to });
          try {
            if (facRelEditId) {
              await ensureEditBackup("修改势力关系");
    await invoke("update_world_setting", { storyId: String(currentStory.id), id: facRelEditId, key: `${facRelDraft.from} → ${facRelDraft.to}`, value: facRelDraft.relation, isHardRule: false, category: "势力关系", metaJson });
              setMsg("✅ 势力关系已更新");
            } else {
              await ensureEditBackup("新建势力关系");
    await invoke("create_world_setting", { storyId: String(currentStory.id), key: `${facRelDraft.from} → ${facRelDraft.to}`, value: facRelDraft.relation, isHardRule: false, category: "势力关系", metaJson });
              setMsg("✅ 势力关系已添加");
            }
            setFacRelEditId(null);
            setFacRelDraft({ from: "", to: "", relation: "中立" });
            await loadWorlds(currentStory.id);
          } catch (e) { setMsg("❌ 保存失败: " + String(e)); }
        };
        const editFacRel = (w: WorldSetting) => { const m = metaOf(w); setFacRelEditId(w.id); setFacRelDraft({ from: m.from || "", to: m.to || "", relation: w.value || "中立" }); };
        const relOf = (a: string, b: string) => facRels.find((r) => { const m = metaOf(r); return m.from === a && m.to === b; });
        // 孤儿势力关系：两端势力已被删除，留在库里但矩阵里渲染不出来（防止"删不掉也看不见"）
        const facNames = new Set(facs.map((f) => f.key));
        const orphanRels = facRels.filter((r) => { const m = metaOf(r); return !facNames.has(m.from || "") || !facNames.has(m.to || ""); });
        const orphanCountOf = (facKey: string) => facRels.filter((r) => { const m = metaOf(r); return m.from === facKey || m.to === facKey; }).length;
        const removeFaction = async (f: WorldSetting) => {
          const n = orphanCountOf(f.key);
          if (n > 0 && !(await confirm({
            title: "删除势力",
            body: `「${f.key}」还有 ${n} 条势力关系。\n\n删除势力后这些关系会变成无效记录（可在矩阵下方查看并清理）。仍要删除？`,
            danger: true,
          }))) return;
          await removeWorld(f.id, f.key);
        };
        const cleanOrphan = async (r: WorldSetting) => {
          const m = metaOf(r);
          const known = (facNames.has(m.from || "") ? m.from : "") || (facNames.has(m.to || "") ? m.to : "");
          const kept = known ? `（保留其中仍存在的一方：${known}）` : "（两端势力都已不存在）";
          if (!(await confirm({
            title: "删除无效的势力关系",
            body: `删除这条无效的势力关系「${m.from} → ${m.to}」${kept}？`,
            danger: true,
          }))) return;
          await removeWorld(r.id, r.key);
        };
        const inputS = { padding: "7px 10px", borderRadius: 7, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontSize: 12, fontFamily: "inherit", width: "100%", boxSizing: "border-box" } as const;
        const labelS = { display: "flex", flexDirection: "column", gap: 4, flex: 1, fontSize: 11.5, color: T.text2 } as const;
        const accentS = { height: 32, padding: "0 14px", borderRadius: 8, border: "none", background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505", cursor: "pointer", fontSize: 12.5, fontWeight: 650, fontFamily: "inherit" } as const;
        const ghostS = { height: 32, padding: "0 12px", borderRadius: 8, border: `1px solid ${T.line}`, background: T.panel2, color: T.text2, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" } as const;
        const miniS = { height: 24, padding: "0 8px", borderRadius: 6, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, cursor: "pointer", fontSize: 10.5, fontFamily: "inherit" } as const;
        const boxS = { border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, padding: 12 } as const;
        const emptyS = { border: `1px dashed ${T.line}`, borderRadius: 12, padding: "44px 20px", textAlign: "center", color: T.text3, fontSize: 12.5, background: T.panel } as const;
        const selLoc = locs.find((l) => l.id === worldSelId) ?? locs[0] ?? null;
        const FAC_COLORS: Record<string, string> = {
          green: "linear-gradient(135deg,#3f8f6d,#1e3d30)",
          blue: "linear-gradient(135deg,#4f8fb8,#22404f)",
          red: "linear-gradient(135deg,#c9584f,#5a2521)",
          purple: "linear-gradient(135deg,#7b5cff,#3a2b6b)",
          gold: "linear-gradient(135deg,#d0913c,#5a3d17)",
        };
        const colorKeyOf = (v?: string) => (v && FAC_COLORS[v] ? v : Object.keys(FAC_COLORS).find((k) => FAC_COLORS[k] === v) || "green");
        const colorBgOf = (v?: string) => (v && FAC_COLORS[v]) || (v && v.startsWith("linear-gradient") ? v : FAC_COLORS.green);
        return (
          <div style={{ maxWidth: 1180, margin: "0 auto" }}>
            <div style={{ display: "flex", alignItems: "flex-start", gap: 12, marginBottom: 14 }}>
              <div>
                <div style={{ fontSize: 15, fontWeight: 650 }}>🌍 世界设定</div>
                <div style={{ fontSize: 11.5, color: T.text3, marginTop: 4 }}>
                  {worlds.length} 条设定 · {new Set(worlds.map((w) => w.category)).size} 个分类 · 硬规则 {hardCount} 条 · {worldConflicts.length > 0 ? <span style={{ color: T.danger }}>{worldConflicts.length} 处冲突</span> : "冲突未检测"}
                </div>
              </div>
              <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
                <button onClick={detect} style={ghostS}>⚠ 冲突检测</button>
                <button disabled title="规划中：AI 补全设定" style={{ ...ghostS, opacity: 0.45, cursor: "not-allowed" }}>✨ AI 补全<span style={{ marginLeft: 4, fontSize: 9.5 }}>规划中</span></button>
                <button onClick={() => runCheck("world")} title="检查设定条目 / 势力矩阵 / 术语的语法与歧义（含本地规则）" style={ghostS}>🔍 检查</button>
                <button onClick={() => resetDraft(worldTab === "map" ? "地理" : worldTab === "factions" ? "势力" : worldTab === "terms" ? "术语" : worldTab === "items" ? "物品" : worldTab === "lore" ? "世界书" : "设定", worldTab === "map" ? { x: "50", y: "50" } : {})} style={accentS}>＋ 新建</button>
              </div>
            </div>
            <div style={{ display: "flex", gap: 4, padding: 3, border: `1px solid ${T.line}`, borderRadius: 10, background: T.panel2, width: "fit-content", marginBottom: 14, flexWrap: "wrap" }}>
              {([["entries", "📚 设定条目"], ["map", "🗺 地图与地点"], ["factions", "🏛 势力矩阵"], ["terms", "📖 术语词典"], ["items", "🎒 物品库"], ["lore", "📖 世界书"], ["conflicts", "⚠ 冲突检测（仅地点）"]] as const).map(([k, lb]) => (
                <button key={k} onClick={() => setWorldTab(k)} style={{ border: "none", borderRadius: 7, padding: "7px 14px", fontSize: 12.5, cursor: "pointer", background: worldTab === k ? T.accentSoft : "transparent", color: worldTab === k ? T.accent : T.text2, fontWeight: worldTab === k ? 600 : 400, fontFamily: "inherit" }}>{lb}</button>
              ))}
            </div>
            {worldTab === "entries" && (
              <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
                <div style={boxS}>
                  <div style={{ display: "flex", gap: 10, flexWrap: "wrap" }}>
                    <label style={labelS}>条目名<input value={wDraft.category === "设定" ? wDraft.key : ""} onChange={(e) => setWDraft({ ...wDraft, category: "设定", key: e.target.value })} placeholder="时代背景 / 魔法体系 / 物理法则" style={inputS} /></label>
                    <label style={labelS}>内容<input value={wDraft.category === "设定" ? wDraft.value : ""} onChange={(e) => setWDraft({ ...wDraft, category: "设定", value: e.target.value })} placeholder="例如：现代都市，无超自然" style={inputS} /></label>
                    <label style={{ ...labelS, flex: "0 0 150px" }}>硬规则<label style={{ display: "flex", alignItems: "center", gap: 6, height: 32, fontSize: 11.5 }}><input type="checkbox" checked={wDraft.category === "设定" && wDraft.hard} onChange={(e) => setWDraft({ ...wDraft, category: "设定", hard: e.target.checked })} />AI 不可违反</label></label>
                  </div>
                  <div style={{ display: "flex", gap: 8, marginTop: 10 }}>
                    <button onClick={saveDraft} style={accentS}>{wEditId && wDraft.category === "设定" ? "保存修改" : "＋ 添加条目"}</button>
                    {wEditId && wDraft.category === "设定" && <button onClick={() => resetDraft("设定")} style={ghostS}>取消编辑</button>}
                  </div>
                </div>
                {plain.length === 0 ? <div style={emptyS}>还没有设定条目。</div> : plain.map((w) => (
                  <div
                    key={w.id}
                    onClick={() => editDraft(w)}
                    title="点击编辑该设定条目"
                    onMouseEnter={(e) => { e.currentTarget.style.borderColor = T.accent; e.currentTarget.style.background = "#20242d"; }}
                    onMouseLeave={(e) => { e.currentTarget.style.borderColor = T.line; e.currentTarget.style.background = T.panel; }}
                    style={{ border: `1px solid ${T.line}`, borderRadius: 10, background: T.panel, padding: "10px 12px", display: "flex", alignItems: "center", gap: 10, cursor: "pointer", transition: "border-color .15s, background .15s" }}
                  >
                    <div style={{ minWidth: 0, flex: 1 }}>
                      <div style={{ fontWeight: 600, fontSize: 13 }}>{w.key}{w.is_hard_rule === 1 && <span style={{ marginLeft: 8, fontSize: 10, background: T.danger, color: "#fff", padding: "1px 6px", borderRadius: 4 }}>硬规则</span>}</div>
                      <div style={{ fontSize: 12, color: T.text3, marginTop: 4 }}>{w.value}</div>
                    </div>
                    <button onClick={(e) => { e.stopPropagation(); editDraft(w); }} style={miniS}>编辑</button>
                    <button onClick={(e) => { e.stopPropagation(); removeWorld(w.id, w.key); }} style={{ ...miniS, color: T.danger }}>删除</button>
                  </div>
                ))}
              </div>
            )}
            {worldTab === "map" && (
              <div style={{ display: "grid", gridTemplateColumns: "minmax(360px,1.2fr) minmax(300px,1fr)", gap: 14, alignItems: "start" }}>
                <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: "#12141a", padding: 10 }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "2px 4px 8px", fontSize: 11.5, color: T.text3 }}>
                    <b style={{ color: T.text2 }}>🗺 地点示意图</b><span>{locs.length} 个地点</span><span style={{ marginLeft: "auto" }}>点图钉 / 右侧条目联动</span>
                  </div>
                  {locs.length === 0 ? (
                    <div style={{ padding: "80px 20px", textAlign: "center", color: T.text3, fontSize: 12.5 }}>还没有地点，先在右侧添加。</div>
                  ) : (
                    <svg viewBox="0 0 600 360" style={{ width: "100%", height: 360, display: "block" }}>
                      <defs><pattern id="wgrid" width="30" height="30" patternUnits="userSpaceOnUse"><path d="M30 0H0V30" fill="none" stroke="#20242d" strokeWidth="1" /></pattern></defs>
                      <rect x="0" y="0" width="600" height="360" fill="url(#wgrid)" />
                      {locs.map((l) => {
                        const m = metaOf(l);
                        const x = 40 + (Number(m.x || 50) / 100) * 520;
                        const y = 30 + (Number(m.y || 50) / 100) * 200;
                        const on = selLoc?.id === l.id;
                        const cur = m.current === "1" || m.current === "true";
                        return (
                          <g key={l.id} onClick={() => setWorldSelId(l.id)} style={{ cursor: "pointer" }}>
                            {cur && <circle cx={x} cy={y} r="15" fill="none" stroke={T.accent} strokeWidth="2" opacity=".85" />}
                            <circle cx={x} cy={y} r={on ? 9 : 7} fill={on ? T.accent : "#5b8cff"} stroke="#12141a" strokeWidth="2" />
                            <text x={x} y={y - 14} textAnchor="middle" fontSize="11" fill={on ? T.accent : "#c8ccd6"} fontWeight={600} paintOrder="stroke" stroke="#12141a" strokeWidth={3}>{l.key}</text>
                          </g>
                        );
                      })}
                    </svg>
                  )}
                </div>
                <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
                  <div style={boxS}>
                    <div style={{ fontSize: 12.5, fontWeight: 650, marginBottom: 8 }}>{wEditId && wDraft.category === "地理" ? "编辑地点" : "新增地点"}</div>
                    <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 8 }}>
                      <label style={labelS}>地点名<input value={wDraft.category === "地理" ? wDraft.key : ""} onChange={(e) => setWDraft({ ...wDraft, category: "地理", key: e.target.value })} style={inputS} /></label>
                      <label style={labelS}>归属势力<input value={wDraft.category === "地理" ? (wDraft.meta.owner || "") : ""} onChange={(e) => setWDraft({ ...wDraft, category: "地理", meta: { ...(wDraft.category === "地理" ? wDraft.meta : {}), owner: e.target.value } })} style={inputS} /></label>
                      <label style={labelS}>区域<input value={wDraft.category === "地理" ? (wDraft.meta.region || "") : ""} onChange={(e) => setWDraft({ ...wDraft, category: "地理", meta: { ...(wDraft.category === "地理" ? wDraft.meta : {}), region: e.target.value } })} style={inputS} /></label>
                      <label style={labelS}>当前场景<select value={wDraft.category === "地理" ? (wDraft.meta.current || "0") : "0"} onChange={(e) => setWDraft({ ...wDraft, category: "地理", meta: { ...(wDraft.category === "地理" ? wDraft.meta : {}), current: e.target.value } })} style={inputS}><option value="0">否</option><option value="1">是</option></select></label>
                      <label style={labelS}>地图 X %<input value={wDraft.category === "地理" ? (wDraft.meta.x || "50") : "50"} onChange={(e) => setWDraft({ ...wDraft, category: "地理", meta: { ...(wDraft.category === "地理" ? wDraft.meta : {}), x: e.target.value } })} style={inputS} /></label>
                      <label style={labelS}>地图 Y %<input value={wDraft.category === "地理" ? (wDraft.meta.y || "50") : "50"} onChange={(e) => setWDraft({ ...wDraft, category: "地理", meta: { ...(wDraft.category === "地理" ? wDraft.meta : {}), y: e.target.value } })} style={inputS} /></label>
                    </div>
                    <label style={{ ...labelS, marginTop: 8 }}>描述<input value={wDraft.category === "地理" ? wDraft.value : ""} onChange={(e) => setWDraft({ ...wDraft, category: "地理", value: e.target.value })} style={inputS} /></label>
                    <div style={{ display: "flex", gap: 8, marginTop: 10 }}>
                      <button onClick={saveDraft} style={accentS}>{wEditId && wDraft.category === "地理" ? "保存修改" : "＋ 添加地点"}</button>
                      <button onClick={() => resetDraft("地理", { x: "50", y: "50" })} style={ghostS}>清空 / 取消</button>
                    </div>
                  </div>
                  <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                    {locs.map((l) => {
                      const m = metaOf(l);
                      const on = selLoc?.id === l.id;
                      return (
                        <div key={l.id} onClick={() => setWorldSelId(l.id)} style={{ border: `1px solid ${on ? T.accent : T.line}`, borderRadius: 10, background: T.panel, padding: "9px 11px", cursor: "pointer" }}>
                          <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 12.5, fontWeight: 600 }}>
                            {l.key}
                            {m.current === "1" && <span style={{ fontSize: 9.5, padding: "1px 6px", borderRadius: 8, background: T.accentSoft, color: T.accent }}>当前场景</span>}
                            <span style={{ marginLeft: "auto" }} />
                            <button onClick={(e) => { e.stopPropagation(); editDraft(l); }} style={miniS}>编辑</button>
                            <button onClick={(e) => { e.stopPropagation(); removeWorld(l.id, l.key); }} style={{ ...miniS, color: T.danger }}>删除</button>
                          </div>
                          <div style={{ fontSize: 11, color: T.text3, marginTop: 4 }}>{[m.region, m.owner].filter(Boolean).join(" · ") || "（未设置区域/归属）"}</div>
                          <div style={{ fontSize: 11.5, color: T.text2, marginTop: 4 }}>{l.value || "（无描述）"}</div>
                        </div>
                      );
                    })}
                  </div>
                </div>
              </div>
            )}
            {worldTab === "factions" && (
              <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
                <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill,minmax(250px,1fr))", gap: 10 }}>
                  {facs.map((f) => {
                    const m = metaOf(f);
                    return (
                      <div
                        key={f.id}
                        onClick={() => { editDraft(f); }}
                        title="点击编辑该势力"
                        onMouseEnter={(e) => { e.currentTarget.style.borderColor = T.accent; e.currentTarget.style.background = "#20242d"; }}
                        onMouseLeave={(e) => { e.currentTarget.style.borderColor = T.line; e.currentTarget.style.background = T.panel; }}
                        style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, overflow: "hidden", cursor: "pointer", transition: "border-color .15s, background .15s" }}
                      >
                        <div style={{ padding: "8px 12px", background: colorBgOf(m.color), color: "#fff", fontWeight: 650, fontSize: 13 }}>{f.key}</div>
                        <div style={{ padding: "10px 12px", display: "flex", flexDirection: "column", gap: 6, fontSize: 11.5, color: T.text2 }}>
                          <div style={{ display: "flex", gap: 8, color: T.text3 }}><span style={{ flex: "0 0 64px" }}>立场</span><span style={{ color: T.text2 }}>{f.value || "—"}</span></div>
                          {([["控制范围", m.control_range], ["规模", m.scale], ["头目", m.leader], ["对主角态度", m.attitude]] as [string, string | undefined][]).filter(([, v]) => v).map(([k, v]) => (
                            <div key={k} style={{ display: "flex", gap: 8, color: T.text3 }}><span style={{ flex: "0 0 64px" }}>{k}</span><span style={{ color: T.text2 }}>{v}</span></div>
                          ))}
                          <div style={{ display: "flex", gap: 6, marginTop: 4 }}>
                            <button onClick={(e) => { e.stopPropagation(); editDraft(f); }} style={miniS}>编辑</button>
                            <button onClick={(e) => { e.stopPropagation(); removeFaction(f); }} style={{ ...miniS, color: T.danger }}>删除</button>
                          </div>
                        </div>
                      </div>
                    );
                  })}
                  {facs.length === 0 && <div style={{ ...emptyS, gridColumn: "1 / -1" }}>还没有势力。</div>}
                </div>
                <div style={boxS}>
                  <div style={{ fontSize: 12.5, fontWeight: 650, marginBottom: 8 }}>{wEditId && wDraft.category === "势力" ? "编辑势力" : "新增势力"}</div>
                  <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: 8 }}>
                    <label style={labelS}>势力名<input value={wDraft.category === "势力" ? wDraft.key : ""} onChange={(e) => setWDraft({ ...wDraft, category: "势力", key: e.target.value })} style={inputS} /></label>
                    <label style={labelS}>立场 / 描述<input value={wDraft.category === "势力" ? wDraft.value : ""} onChange={(e) => setWDraft({ ...wDraft, category: "势力", value: e.target.value })} style={inputS} /></label>
                    <label style={labelS}>主色<select value={wDraft.category === "势力" ? colorKeyOf(wDraft.meta.color) : "green"} onChange={(e) => setWDraft({ ...wDraft, category: "势力", meta: { ...(wDraft.category === "势力" ? wDraft.meta : {}), color: e.target.value } })} style={inputS}>{([["green", "绿色"], ["blue", "蓝色"], ["red", "红色"], ["purple", "紫色"], ["gold", "金色"]] as const).map(([k, lb]) => <option key={k} value={k}>{lb}</option>)}</select></label>
                    <label style={labelS}>控制范围<input value={wDraft.category === "势力" ? (wDraft.meta.control_range || "") : ""} onChange={(e) => setWDraft({ ...wDraft, category: "势力", meta: { ...(wDraft.category === "势力" ? wDraft.meta : {}), control_range: e.target.value } })} style={inputS} /></label>
                    <label style={labelS}>规模<input value={wDraft.category === "势力" ? (wDraft.meta.scale || "") : ""} onChange={(e) => setWDraft({ ...wDraft, category: "势力", meta: { ...(wDraft.category === "势力" ? wDraft.meta : {}), scale: e.target.value } })} style={inputS} /></label>
                    <label style={labelS}>头目<input value={wDraft.category === "势力" ? (wDraft.meta.leader || "") : ""} onChange={(e) => setWDraft({ ...wDraft, category: "势力", meta: { ...(wDraft.category === "势力" ? wDraft.meta : {}), leader: e.target.value } })} style={inputS} /></label>
                    <label style={labelS}>对主角态度<input value={wDraft.category === "势力" ? (wDraft.meta.attitude || "") : ""} onChange={(e) => setWDraft({ ...wDraft, category: "势力", meta: { ...(wDraft.category === "势力" ? wDraft.meta : {}), attitude: e.target.value } })} style={inputS} /></label>
                    <div style={{ gridColumn: "1 / -1", display: "flex", gap: 8 }}>
                      <button onClick={saveDraft} style={accentS}>{wEditId && wDraft.category === "势力" ? "保存修改" : "＋ 添加势力"}</button>
                      <button onClick={() => resetDraft("势力")} style={ghostS}>清空 / 取消</button>
                    </div>
                  </div>
                </div>
                <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, overflow: "hidden" }}>
                  <div style={{ padding: "10px 12px", background: T.panel2, borderBottom: `1px solid ${T.lineSoft}`, fontSize: 12 }}>势力关系矩阵 · {facRels.length} 条关系</div>
                  {facs.length < 2 ? (
                    <div style={{ padding: "30px 14px", textAlign: "center", color: T.text3, fontSize: 12 }}>至少 2 个势力才能形成矩阵。</div>
                  ) : (
                    <div style={{ overflowX: "auto", padding: 12 }}>
                      <table style={{ borderCollapse: "collapse", width: "100%", fontSize: 11 }}>
                        <thead><tr><th></th>{facs.map((f) => <th key={f.id} style={{ padding: 6, color: T.text3, fontWeight: 500 }}>{f.key}</th>)}</tr></thead>
                        <tbody>
                          {facs.map((a) => (
                            <tr key={a.id}>
                              <td style={{ padding: 6, color: T.text3, whiteSpace: "nowrap" }}>{a.key}</td>
                              {facs.map((b) => {
                                if (a.id === b.id) return <td key={b.id} style={{ padding: 6, textAlign: "center", color: T.text3 }}>—</td>;
                                const rel = relOf(a.key, b.key);
                                const label = rel?.value || "";
                                const color = label.includes("敌") ? T.danger : label.includes("同盟") ? T.ok : label.includes("紧张") ? T.warn : T.text3;
                                return (
                                  <td key={b.id} style={{ padding: 4, textAlign: "center" }}>
                                    <span onClick={() => { if (rel) editFacRel(rel); else { setFacRelEditId(null); setFacRelDraft({ from: a.key, to: b.key, relation: "中立" }); } }} style={{ display: "inline-block", cursor: "pointer", fontSize: 10.5, padding: "2px 8px", borderRadius: 9, border: `1px solid ${label ? color + "66" : T.line}`, background: label ? color + "18" : "#191c22", color: label ? color : T.text3 }}>{label || "＋"}</span>
                                  </td>
                                );
                              })}
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  )}
                  <div style={{ display: "flex", gap: 8, alignItems: "flex-end", padding: "0 12px 12px", flexWrap: "wrap" }}>
                    <label style={labelS}>从<select value={facRelDraft.from} onChange={(e) => setFacRelDraft({ ...facRelDraft, from: e.target.value })} style={inputS}><option value="">—</option>{facs.map((f) => <option key={f.id} value={f.key}>{f.key}</option>)}</select></label>
                    <label style={labelS}>到<select value={facRelDraft.to} onChange={(e) => setFacRelDraft({ ...facRelDraft, to: e.target.value })} style={inputS}><option value="">—</option>{facs.map((f) => <option key={f.id} value={f.key}>{f.key}</option>)}</select></label>
                    <label style={labelS}>关系<select value={facRelDraft.relation} onChange={(e) => setFacRelDraft({ ...facRelDraft, relation: e.target.value })} style={inputS}>{["敌对", "紧张", "冷处理", "同盟", "中立"].map((x) => <option key={x} value={x}>{x}</option>)}</select></label>
                    <button onClick={saveFacRel} style={accentS}>{facRelEditId ? "保存关系" : "＋ 设置关系"}</button>
                    {facRelEditId && <button onClick={async () => { const rel = facRels.find((x) => x.id === facRelEditId); if (rel) await removeWorld(rel.id, rel.key); setFacRelEditId(null); setFacRelDraft({ from: "", to: "", relation: "中立" }); }} style={{ ...ghostS, color: T.danger }}>删除关系</button>}
                    {facRelEditId && <button onClick={() => { setFacRelEditId(null); setFacRelDraft({ from: "", to: "", relation: "中立" }); }} style={ghostS}>取消</button>}
                  </div>
                  {orphanRels.length > 0 && (
                    <div style={{ borderTop: `1px solid ${T.lineSoft}`, padding: "10px 12px", background: "rgba(240,85,77,.045)" }}>
                      <div style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 11.5, color: "#ff9d97", marginBottom: 8 }}>
                        <span>⚠ 无效势力关系 {orphanRels.length} 条（关联的势力已被删除，矩阵里显示不出来）</span>
                      </div>
                      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                        {orphanRels.map((r) => {
                          const m = metaOf(r);
                          const missing = [m.from, m.to].filter((x) => !facNames.has(x || "")).join("、");
                          return (
                            <div key={r.id} style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 11, background: T.panel2, border: `1px solid ${T.line}`, borderRadius: 8, padding: "6px 10px" }}>
                              <span style={{ color: T.text2 }}>{m.from} <span style={{ color: T.text3 }}>→</span> {m.to}</span>
                              <span style={{ color: T.text3 }}>· {r.value || "中立"}</span>
                              <span style={{ marginLeft: "auto", color: T.text3 }}>缺失：{missing || "—"}</span>
                              <button onClick={() => cleanOrphan(r)} style={{ ...miniS, color: T.danger }}>删除这条</button>
                            </div>
                          );
                        })}
                      </div>
                      <div style={{ fontSize: 10.5, color: T.text3, marginTop: 8 }}>清理建议：先重建同名势力，再到矩阵里重设关系；或直接删掉这些无效记录。</div>
                    </div>
                  )}
                </div>
              </div>
            )}
            {worldTab === "terms" && (
              <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
                <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
                  <input value={worldSearch} onChange={(e) => setWorldSearch(e.target.value)} placeholder="搜索术语 / 别名 / 释义" style={{ ...inputS, width: 260 }} />
                  <span style={{ fontSize: 11.5, color: T.text3 }}>共 {terms.length} 个词条</span>
                </div>
                <div style={boxS}>
                  <div style={{ fontSize: 12.5, fontWeight: 650, marginBottom: 8 }}>{wEditId && wDraft.category === "术语" ? "编辑词条" : "新增词条"}</div>
                  <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 8 }}>
                    <label style={labelS}>术语<input value={wDraft.category === "术语" ? wDraft.key : ""} onChange={(e) => setWDraft({ ...wDraft, category: "术语", key: e.target.value })} style={inputS} /></label>
                    <label style={labelS}>别名（逗号分隔）<input value={wDraft.category === "术语" ? (wDraft.meta.aliases || "") : ""} onChange={(e) => setWDraft({ ...wDraft, category: "术语", meta: { ...(wDraft.category === "术语" ? wDraft.meta : {}), aliases: e.target.value } })} style={inputS} /></label>
                  </div>
                  <label style={{ ...labelS, marginTop: 8 }}>释义<input value={wDraft.category === "术语" ? wDraft.value : ""} onChange={(e) => setWDraft({ ...wDraft, category: "术语", value: e.target.value })} style={inputS} /></label>
                  <label style={{ ...labelS, marginTop: 8 }}>出现章节<input value={wDraft.category === "术语" ? (wDraft.meta.chapters || "") : ""} onChange={(e) => setWDraft({ ...wDraft, category: "术语", meta: { ...(wDraft.category === "术语" ? wDraft.meta : {}), chapters: e.target.value } })} style={inputS} /></label>
                  <div style={{ display: "flex", gap: 8, marginTop: 10 }}>
                    <button onClick={saveDraft} style={accentS}>{wEditId && wDraft.category === "术语" ? "保存修改" : "＋ 添加词条"}</button>
                    <button onClick={() => resetDraft("术语")} style={ghostS}>清空 / 取消</button>
                  </div>
                </div>
                <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, overflow: "hidden" }}>
                  <div style={{ display: "grid", gridTemplateColumns: "160px 150px 1fr 130px 100px", gap: 10, padding: "9px 22px", background: T.panel2, fontSize: 10.5, color: T.text3 }}>
                    <span>术语</span><span>别名</span><span>释义</span><span>出现章节</span><span style={{ textAlign: "right" }}>操作</span>
                  </div>
                  {terms.filter((w) => { const m = metaOf(w); const q = worldSearch.trim().toLowerCase(); return !q || w.key.toLowerCase().includes(q) || (m.aliases || "").toLowerCase().includes(q) || w.value.toLowerCase().includes(q); }).map((w) => {
                    const m = metaOf(w);
                    return (
                      <div
                        key={w.id}
                        onClick={() => editDraft(w)}
                        title="点击编辑该词条"
                        onMouseEnter={(e) => { e.currentTarget.style.borderColor = T.accent; e.currentTarget.style.background = "#20242d"; }}
                        onMouseLeave={(e) => { e.currentTarget.style.borderColor = T.line; e.currentTarget.style.background = T.panel; }}
                        style={{ display: "grid", gridTemplateColumns: "160px 150px 1fr 130px 100px", gap: 10, padding: "9px 12px", margin: "6px 10px 0", border: `1px solid ${T.line}`, borderRadius: 10, fontSize: 11.5, alignItems: "center", cursor: "pointer", transition: "border-color .15s, background .15s" }}
                      >
                        <span style={{ color: T.text1, fontWeight: 600 }}>{w.key}</span>
                        <span style={{ color: T.text3 }}>{m.aliases || "—"}</span>
                        <span style={{ color: T.text2 }}>{w.value}</span>
                        <span style={{ color: T.text3 }}>{m.chapters || "—"}</span>
                        <span style={{ textAlign: "right" }}><button onClick={(e) => { e.stopPropagation(); editDraft(w); }} style={miniS}>编辑</button> <button onClick={(e) => { e.stopPropagation(); removeWorld(w.id, w.key); }} style={{ ...miniS, color: T.danger }}>删除</button></span>
                      </div>
                    );
                  })}
                  {terms.length === 0 && <div style={{ padding: "26px 14px", textAlign: "center", color: T.text3, fontSize: 12 }}>还没有术语词条。</div>}
                </div>
              </div>
            )}
            {worldTab === "items" && (
              <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
                <div style={{ fontSize: 11.5, color: T.text3, lineHeight: 1.7, border: `1px dashed ${T.line}`, borderRadius: 10, padding: "8px 12px" }}>
                  <b style={{ color: T.text2 }}>🎒 物品库</b>：放"出现的勤的东西"——能拿在手里的（怀表、钥匙、药瓶、信物）。
                  概念 / 组织 / 地名请放「📖 术语词典」。<br />
                  物品会**全量注入**提示词（一行一条），并参与「🔍 回顾」的实体召回。
                </div>
                <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
                  <input value={worldSearch} onChange={(e) => setWorldSearch(e.target.value)} placeholder="搜索物品名 / 别名 / 说明" style={{ flex: "1 1 260px", maxWidth: 380, height: 32, padding: "0 11px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontSize: 12.5, outline: "none" }} />
                  <span style={{ fontSize: 11.5, color: T.text3 }}>共 {items.length} 件</span>
                </div>
                <div style={boxS}>
                  <div style={{ fontSize: 12.5, fontWeight: 650, marginBottom: 8 }}>{wEditId && wDraft.category === "物品" ? "✏️ 编辑物品" : "＋ 添加物品"}</div>
                  <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 8 }}>
                    <label style={labelS}>物品名<input value={wDraft.category === "物品" ? wDraft.key : ""} onChange={(e) => setWDraft({ ...wDraft, category: "物品", key: e.target.value })} placeholder="怀表" style={inputS} /></label>
                    <label style={labelS}>别名（逗号分隔）<input value={wDraft.category === "物品" ? (wDraft.meta.aliases || "") : ""} onChange={(e) => setWDraft({ ...wDraft, category: "物品", meta: { ...wDraft.meta, aliases: e.target.value } })} placeholder="旧表，那块表" style={inputS} /></label>
                  </div>
                  <label style={{ ...labelS, marginTop: 8 }}>说明<input value={wDraft.category === "物品" ? wDraft.value : ""} onChange={(e) => setWDraft({ ...wDraft, category: "物品", value: e.target.value })} placeholder="B 送的，走时慢三分钟；她说修不好了" style={inputS} /></label>
                  <div style={{ display: "flex", gap: 8, marginTop: 10 }}>
                    <button onClick={saveDraft} style={accentS}>{wEditId && wDraft.category === "物品" ? "保存修改" : "＋ 添加物品"}</button>
                    <button onClick={() => resetDraft("物品")} style={ghostS}>清空 / 取消</button>
                  </div>
                </div>
                <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, overflow: "hidden" }}>
                  <div style={{ display: "grid", gridTemplateColumns: "180px 150px 1fr 120px", gap: 10, padding: "9px 14px", fontSize: 11, color: T.text3, borderBottom: `1px solid ${T.line}`, background: "#12141a" }}>
                    <span>物品</span><span>别名</span><span>说明</span><span style={{ textAlign: "right" }}>操作</span>
                  </div>
                  {items.filter((w) => {
                    const q = worldSearch.trim().toLowerCase();
                    if (!q) return true;
                    const m = metaOf(w);
                    return w.key.toLowerCase().includes(q)
                      || (w.value || "").toLowerCase().includes(q)
                      || (m.aliases || "").toLowerCase().includes(q);
                  }).map((w) => {
                    const m = metaOf(w);
                    return (
                      <div
                        key={w.id}
                        onClick={() => editDraft(w)}
                        title="点击编辑该物品"
                        style={{ display: "grid", gridTemplateColumns: "180px 150px 1fr 120px", gap: 10, padding: "9px 14px", fontSize: 12, borderTop: `1px solid ${T.lineSoft}`, cursor: "pointer" }}
                      >
                        <span style={{ color: T.text1, fontWeight: 600 }}>{w.key}</span>
                        <span style={{ color: T.text3 }}>{m.aliases || "—"}</span>
                        <span style={{ color: T.text2 }}>{w.value}</span>
                        <span style={{ textAlign: "right" }}>
                          <button onClick={(e) => { e.stopPropagation(); editDraft(w); }} style={miniS}>编辑</button>
                          <button onClick={(e) => { e.stopPropagation(); removeWorld(w.id, w.key); }} style={{ ...miniS, color: T.danger }}>删除</button>
                        </span>
                      </div>
                    );
                  })}
                  {items.length === 0 && (
                    <div style={{ padding: "26px 14px", textAlign: "center", color: T.text3, fontSize: 12 }}>
                      还没有物品。上面填「物品名 + 说明」就能加（别名可选）。
                    </div>
                  )}
                </div>
              </div>
            )}
            {worldTab === "lore" && (
              <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
                <div style={{ fontSize: 11.5, color: T.text3, lineHeight: 1.75, border: `1px dashed ${T.line}`, borderRadius: 10, padding: "8px 12px" }}>
                  <b style={{ color: T.text2 }}>📖 世界书</b>：放"多而散、不必每轮都在场"的设定（人物前史、器物来历、某地习俗）。<br />
                  <b>和物品库/术语的区别</b>：那些是<b>全量</b>进提示词的（少而稳）；世界书只在<b>触发词命中</b>时才进，命中后放在尾部块（不吃前缀缓存）。<br />
                  触发范围：本次输入 + 最近 3 条消息；都命中时按<b>优先级</b>排，总字数上限约 1200 字，超出的会注明"另有 N 条未注入"。
                </div>
                <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
                  <input value={worldSearch} onChange={(e) => setWorldSearch(e.target.value)} placeholder="搜索条目 / 触发词 / 内容" style={{ flex: "1 1 240px", maxWidth: 360, height: 32, padding: "0 11px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontSize: 12.5, outline: "none" }} />
                  <span style={{ fontSize: 11.5, color: T.text3 }}>共 {lores.length} 条</span>
                </div>
                <div style={boxS}>
                  <div style={{ fontSize: 12.5, fontWeight: 650, marginBottom: 8 }}>{wEditId && wDraft.category === "世界书" ? "✏️ 编辑条目" : "＋ 添加条目"}</div>
                  <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 120px", gap: 8 }}>
                    <label style={labelS}>条目名<input value={wDraft.category === "世界书" ? wDraft.key : ""} onChange={(e) => setWDraft({ ...wDraft, category: "世界书", key: e.target.value })} placeholder="怀表的来历" style={inputS} /></label>
                    <label style={labelS}>触发词（逗号分隔）<input value={wDraft.category === "世界书" ? (wDraft.meta.triggers || "") : ""} onChange={(e) => setWDraft({ ...wDraft, category: "世界书", meta: { ...wDraft.meta, triggers: e.target.value } })} placeholder="怀表,旧表" style={inputS} /></label>
                    <label style={labelS}>优先级<input value={wDraft.category === "世界书" ? (wDraft.meta.priority || "50") : "50"} onChange={(e) => setWDraft({ ...wDraft, category: "世界书", meta: { ...wDraft.meta, priority: e.target.value } })} placeholder="50" style={inputS} /></label>
                  </div>
                  <label style={{ ...labelS, marginTop: 8 }}>内容（命中后注入）<textarea value={wDraft.category === "世界书" ? wDraft.value : ""} onChange={(e) => setWDraft({ ...wDraft, category: "世界书", value: e.target.value })} rows={3} placeholder="例：这块表是 B 母亲留下的，机芯修过三次，走时慢三分钟；B 从不让人碰。" style={{ ...inputS, resize: "vertical", lineHeight: 1.65 }} /></label>
                  <label style={{ ...labelS, marginTop: 8, flexDirection: "row", alignItems: "center", gap: 8 }}>
                    <input type="checkbox" checked={wDraft.category === "世界书" && wDraft.meta.always === "1"} onChange={(e) => setWDraft({ ...wDraft, category: "世界书", meta: { ...wDraft.meta, always: e.target.checked ? "1" : "0" } })} />
                    <span>常驻（不看触发词，每轮都注入）</span>
                  </label>
                  <div style={{ display: "flex", gap: 8, marginTop: 10 }}>
                    <button onClick={saveDraft} style={accentS}>{wEditId && wDraft.category === "世界书" ? "保存修改" : "＋ 添加条目"}</button>
                    <button onClick={() => resetDraft("世界书")} style={ghostS}>清空 / 取消</button>
                  </div>
                </div>
                <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, overflow: "hidden" }}>
                  <div style={{ display: "grid", gridTemplateColumns: "180px 170px 1fr 90px 110px", gap: 10, padding: "9px 14px", fontSize: 11, color: T.text3, borderBottom: `1px solid ${T.line}`, background: "#12141a" }}>
                    <span>条目</span><span>触发词</span><span>内容</span><span>优先级</span><span style={{ textAlign: "right" }}>操作</span>
                  </div>
                  {lores.filter((w) => {
                    const q = worldSearch.trim().toLowerCase();
                    if (!q) return true;
                    const m = metaOf(w);
                    return w.key.toLowerCase().includes(q)
                      || (w.value || "").toLowerCase().includes(q)
                      || (m.triggers || "").toLowerCase().includes(q);
                  }).map((w) => {
                    const m = metaOf(w);
                    const always = m.always === "1";
                    return (
                      <div
                        key={w.id}
                        onClick={() => editDraft(w)}
                        title="点击编辑该条目"
                        style={{ display: "grid", gridTemplateColumns: "180px 170px 1fr 90px 110px", gap: 10, padding: "9px 14px", fontSize: 12, borderTop: `1px solid ${T.lineSoft}`, cursor: "pointer" }}
                      >
                        <span style={{ color: T.text1, fontWeight: 600 }}>
                          {w.key}
                          {always && <span style={{ marginLeft: 6, fontSize: 10, padding: "1px 6px", borderRadius: 6, border: `1px solid ${T.accent}66`, color: T.accent }}>常驻</span>}
                        </span>
                        <span style={{ color: T.text3 }}>{m.triggers || (always ? "—（常驻）" : "—")}</span>
                        <span style={{ color: T.text2, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{w.value}</span>
                        <span style={{ color: T.text3 }}>{m.priority || "50"}</span>
                        <span style={{ textAlign: "right" }}>
                          <button onClick={(e) => { e.stopPropagation(); editDraft(w); }} style={miniS}>编辑</button>
                          <button onClick={(e) => { e.stopPropagation(); removeWorld(w.id, w.key); }} style={{ ...miniS, color: T.danger }}>删除</button>
                        </span>
                      </div>
                    );
                  })}
                  {lores.length === 0 && (
                    <div style={{ padding: "26px 14px", textAlign: "center", color: T.text3, fontSize: 12 }}>
                      还没有条目。上面填「条目名 + 触发词 + 内容」就能加；不写触发词又想每轮都在场，就勾「常驻」。
                    </div>
                  )}
                </div>
              </div>
            )}
            {worldTab === "conflicts" && (
              <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
                <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                  <button onClick={detect} style={accentS}>⚠ 开始冲突检测</button>
                  <span style={{ fontSize: 11.5, color: T.text3 }}>当前仅检测「地图与地点」坐标重复（同一 x,y 被两个地点使用）；设定条目冲突检测暂未覆盖，标记为后期修正</span>
                </div>
                {worldLoading ? (
                  <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
                    {[0, 1].map((i) => <div key={i} style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, padding: 14 }}><div style={{ height: 12, width: "38%", background: "#2a2f3a", borderRadius: 4, marginBottom: 10 }} /><div style={{ height: 60, background: "#1e222a", borderRadius: 8 }} /></div>)}
                  </div>
                ) : worldConflicts.length === 0 ? (
                  <div style={emptyS}>✅ 当前没有检测到设定冲突。点上方按钮重新检测。</div>
                ) : (
                  worldConflicts.map((c, i) => (
                    <div key={i} style={{ border: `1px solid ${c.level === "严重" ? T.danger + "66" : T.line}`, borderRadius: 12, background: T.panel, overflow: "hidden" }}>
                      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "10px 12px", background: T.panel2, fontSize: 12.5 }}>
                        <span>⚠</span><b>{c.title}</b>
                        <span style={{ marginLeft: "auto", fontSize: 10, padding: "1px 8px", borderRadius: 8, color: c.level === "严重" ? T.danger : T.warn, border: `1px solid ${c.level === "严重" ? T.danger + "66" : T.warn + "66"}` }}>{c.level}</span>
                      </div>
                      <div style={{ display: "grid", gridTemplateColumns: "1fr 1px 1fr", gap: 10, padding: 12 }}>
                        <div><div style={{ fontSize: 10.5, color: T.text3, marginBottom: 4 }}>{c.left_label}</div><div style={{ fontSize: 11.5, color: T.text2, lineHeight: 1.7 }}>{c.left_text}</div><div style={{ fontSize: 10, color: T.text3, marginTop: 6 }}>{c.left_source}</div></div>
                        <div style={{ background: T.lineSoft }} />
                        <div><div style={{ fontSize: 10.5, color: T.text3, marginBottom: 4 }}>{c.right_label}</div><div style={{ fontSize: 11.5, color: T.text2, lineHeight: 1.7 }}>{c.right_text}</div><div style={{ fontSize: 10, color: T.text3, marginTop: 6 }}>{c.right_source}</div></div>
                      </div>
                      <div style={{ display: "flex", gap: 8, padding: "0 12px 12px" }}>
                        <button onClick={() => dropEntry(c.right_id, c.title)} style={ghostS}>保留左侧</button>
                        <button onClick={() => dropEntry(c.left_id, c.title)} style={ghostS}>保留右侧</button>
                        <button onClick={() => setMsg("已暂时忽略该冲突")} style={ghostS}>暂时忽略</button>
                      </div>
                    </div>
                  ))
                )}
              </div>
            )}
          </div>
        );
      })()
  );
}
