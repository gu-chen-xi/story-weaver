// 「👥 角色」页（角色卡 + 关系图 + 关系列表与编辑）。
//
// 拆分说明（2026-10-17）：从 App.tsx **整体下沉** —— 视图 + 30 余个立绘/角色专用 state
// 和处理函数都搬进来了，只留 13 个跨页共享项作为 props（当前故事、角色列表、
// 关系列表及其加载器、全局提示等）。
//
// 与立绘页不同的是：这次**一次做到位**（视图与状态一起搬），因为角色页的 state
// 只被本页和本页的处理函数使用，外部只有"角色列表/关系列表"需要共享。

import { useState, type Dispatch, type SetStateAction } from "react";
import type { AppView } from "../types";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { T } from "../theme";
import { EMPTY_CHAR_FORM, REL_STATES, REL_STATE_HINT } from "../constants";
import { normRelState } from "../lib/parse";
import { charNameOf, emotionInfo } from "../lib/chars";
import type { Character, CharacterForm, CharacterUsage, PortraitItem, Relation, Story } from "../types";
import { useConfirm } from "../confirm";

/// 顶部导航的视图名（与 App 的 view state 保持一致）
export type ViewName = AppView;

export type CharsPageProps = {
  charGraphRootId: number | null;
  chars: Character[];
  charsLoading: boolean;
  /// 角色使用统计（角色页要显示出场次数；App 的 loadChars 也会写它）
  charUsage: CharacterUsage[];
  currentStory: Story | null;
  ensureEditBackup: (what: string) => Promise<void>;
  loadChars: (storyId: number) => Promise<void>;
  loadRelations: (storyId: number) => Promise<void>;
  portraits: PortraitItem[];
  relations: Relation[];
  runCheck: (board: "characters" | "world") => Promise<void>;
  setCharGraphRootId: Dispatch<SetStateAction<number | null>>;
  setMsg: Dispatch<SetStateAction<string>>;
  setView: Dispatch<SetStateAction<ViewName>>;
};

export function CharsPage(props: CharsPageProps) {
  const confirm = useConfirm();
  const { charGraphRootId, chars, charsLoading, charUsage, currentStory, ensureEditBackup, loadChars, loadRelations, portraits, relations, runCheck, setCharGraphRootId, setMsg, setView } = props;
  // ===== 以下为从 App.tsx 搬进来的角色页状态与处理函数 =====
  const [charTab, setCharTab] = useState<"cards" | "relations">("cards");
  const [charSearch, setCharSearch] = useState("");
  const [charFilter, setCharFilter] = useState("全部");
  const [charFormOpen, setCharFormOpen] = useState(false);
  const [charEditingId, setCharEditingId] = useState<number | null>(null);
  const [charError, setCharError] = useState("");
  const [charForm, setCharForm] = useState<CharacterForm>({ ...EMPTY_CHAR_FORM });
  const [rFrom, setRFrom] = useState<number>(0);
  const [rTo, setRTo] = useState<number>(0);
  const [rType, setRType] = useState("陌生人");
  const [rDir, setRDir] = useState<"单向" | "双向">("单向");
  const [rStrength, setRStrength] = useState(50);
  const [rState, setRState] = useState("稳定");
  const [rDescription, setRDescription] = useState("");
  const [rEvidence, setREvidence] = useState("");
  const [relEditingId, setRelEditingId] = useState<number | null>(null);
  const [relExpandId, setRelExpandId] = useState<number | null>(null);
  const openCharForm = (c: Character | null) => {
    setCharError("");
    if (c) {
      setCharEditingId(c.id);
      setCharForm({
        name: c.name, alias: c.alias, role: c.role, faction: c.faction, age: c.age, gender: c.gender,
        first_appearance: c.first_appearance, one_liner: c.one_liner, profile: c.profile,
        speech_style: c.speech_style, traits: c.traits,
        background_past: c.background_past ?? "", appearance: c.appearance ?? "", motivation: c.motivation ?? "", goal: c.goal ?? "",
        dialogue_examples: c.dialogue_examples ?? "", behavior_rules: c.behavior_rules ?? "",
        stat_stamina: c.stat_stamina, stat_sanity: c.stat_sanity, stat_emotion: c.stat_emotion, stat_screen: c.stat_screen,
      });
    } else {
      setCharEditingId(null);
      setCharForm({ ...EMPTY_CHAR_FORM });
    }
    setCharFormOpen(true);
  };
  const saveCharForm = async () => {
    if (!currentStory) return;
    if (!charForm.name.trim()) { setCharError("角色名不能为空"); return; }
    if (!charForm.gender.trim()) { setCharError("性别为必填项，请先填写性别"); return; }
    const input = { ...charForm, name: charForm.name.trim() };
    await ensureEditBackup(charEditingId ? `修改角色「${input.name}」` : `新建角色「${input.name}」`);
    try {
      if (charEditingId) {
        await invoke("update_character", { storyId: String(currentStory.id), id: charEditingId, input });
        setMsg(`✅ 已更新角色「${input.name}」`);
      } else {
        await invoke("create_character", { storyId: String(currentStory.id), input });
        setMsg(`✅ 已新建角色「${input.name}」`);
      }
      setCharFormOpen(false);
      setCharError("");
      await loadChars(currentStory.id);
    } catch (e) {
      setCharError(String(e));
    }
  };
  const randomCharForm = () => {
    const pick = (arr: string[]) => arr[Math.floor(Math.random() * arr.length)];
    setCharForm((f) => ({
      ...f,
      traits: f.traits || pick(["克制", "偏执", "责任感过重", "冷幽默", "温柔", "警惕", "乐观"]),
      stat_stamina: 30 + Math.floor(Math.random() * 60),
      stat_sanity: 30 + Math.floor(Math.random() * 60),
      stat_emotion: 30 + Math.floor(Math.random() * 60),
      stat_screen: 10 + Math.floor(Math.random() * 80),
    }));
  };
  const removeChar = async (id: number, name: string) => {
    if (!currentStory) return;
    if (!(await confirm({
      title: "删除角色",
      body: `删除角色「${name}」？\n\n与它相关的关系记录也会一起清掉，不可撤销。`,
      danger: true,
    }))) return;
    try {
      await ensureEditBackup("删除角色");
      await invoke("delete_character", { storyId: String(currentStory.id), id });
      setMsg(`🗑️ 已删除角色「${name}」`);
      await loadChars(currentStory.id);
    } catch (e) {
      setMsg("❌ 删除失败: " + String(e));
    }
  };
  const resetRelationForm = () => {
    setRelEditingId(null);
    setRType("陌生人");
    setRStrength(50);
    setRState("稳定");
    setRDescription("");
    setREvidence("");
  };
  const saveRelation = async () => {
    if (!currentStory) return;
    if (rFrom === 0 || rTo === 0) { setMsg("❌ 请选择两个角色"); return; }
    if (rFrom === rTo) { setMsg("❌ 两个角色不能是同一个"); return; }
    const relationType = rType.trim();
    const state = rState.trim() || "稳定";
    const description = rDescription.trim();
    const evidence = rEvidence.trim();
    try {
      if (relEditingId) {
        await ensureEditBackup("修改关系");
      await invoke("update_relation", {
          storyId: String(currentStory.id), id: relEditingId,
          relationType, direction: rDir, strength: rStrength, state, description, evidence,
        });
        setMsg("✅ 已更新关系");
      } else {
        await ensureEditBackup("新建关系");
      await invoke("create_relation", {
          storyId: String(currentStory.id), fromId: rFrom, toId: rTo,
          relationType, direction: rDir, strength: rStrength, state, description, evidence,
        });
        setMsg("✅ 已添加关系");
      }
      resetRelationForm();
      await loadRelations(currentStory.id);
    } catch (e) {
      setMsg((relEditingId ? "❌ 更新关系失败: " : "❌ 添加关系失败: ") + String(e));
    }
  };
  const editRelation = (r: Relation) => {
    setRelEditingId(r.id);
    setRFrom(r.from_character_id);
    setRTo(r.to_character_id);
    setRType(r.relation_type || "陌生人");
    setRDir(r.direction === "双向" ? "双向" : "单向");
    setRStrength(r.strength);
    setRState(normRelState(r.state));
    setRDescription(r.description || "");
    setREvidence(r.evidence || "");
    setCharTab("relations");
  };
  const removeRelation = async (id: number) => {
    if (!currentStory) return;
    if (!(await confirm({ title: "删除关系", body: "删除这条关系？", danger: true }))) return;
    try {
      await ensureEditBackup("删除关系");
      await invoke("delete_relation", { storyId: String(currentStory.id), id });
      setMsg("🗑️ 已删除关系");
      await loadRelations(currentStory.id);
    } catch (e) {
      setMsg("❌ 删除失败: " + String(e));
    }
  };
  return (
      (() => {
        const usageOf = (id: number) => charUsage.find((u) => u.id === id)?.dialogue_count ?? 0;
        const nodesOf = (id: number) => charUsage.find((u) => u.id === id)?.node_count ?? 0;
        const portraitsOf = (id: number) => portraits.filter((p) => p.character_id === id && p.story_id === currentStory?.id);
        const relCountOf = (id: number) => relations.filter((r) => r.from_character_id === id || r.to_character_id === id).length;
        const roleOptions = ["全部", "未定位", "男主", "女主", "对手", "配角", "背景", "其他", "主角"];
        const filteredChars = chars.filter((c) => {
          const q = charSearch.trim().toLowerCase();
          const hitQ = !q || [c.name, c.alias, c.role, c.faction, c.one_liner].some((s) => (s || "").toLowerCase().includes(q));
          const hitF = charFilter === "全部" || (c.role || "未定位") === charFilter;
          return hitQ && hitF;
        });
        const coverChars = new Set(portraits.filter((p) => p.character_id && p.story_id === currentStory?.id).map((p) => p.character_id)).size;
        const missingCover = chars.filter((c) => portraitsOf(c.id).length === 0).length;
        const labelStyle = { display: "flex", flexDirection: "column", gap: 4, flex: 1, fontSize: 11.5, color: T.text2 } as const;
        const inputStyle = { padding: "7px 10px", borderRadius: 7, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontSize: 12, fontFamily: "inherit", width: "100%", boxSizing: "border-box" } as const;
        const miniBtn = { height: 24, padding: "0 8px", borderRadius: 6, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, cursor: "pointer", fontSize: 10.5, fontFamily: "inherit" } as const;
        const accentBtn = { height: 32, padding: "0 14px", borderRadius: 8, border: "none", background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505", cursor: "pointer", fontSize: 12.5, fontWeight: 650, fontFamily: "inherit" } as const;
        const ghostBtn = { height: 32, padding: "0 12px", borderRadius: 8, border: `1px solid ${T.line}`, background: T.panel2, color: T.text2, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" } as const;
        const relNature = (r: Relation) => {
          const s = `${r.relation_type}${r.state}`;
          if (/敌|仇|对立|对抗|宿敌|杀/.test(s)) return { color: T.danger, dash: "", label: "敌对" };
          if (/信任|朋友|友|亲人|家人|兄妹|恋人|爱/.test(s)) return { color: T.ok, dash: "5 4", label: "可信任" };
          if (/待确认|未明|动摇|存疑|未知/.test(r.state)) return { color: "#5b8cff", dash: "4 4", label: "存疑" };
          if (r.strength >= 60) return { color: "#ff9a45", dash: "", label: "强关联" };
          return { color: "#6f7686", dash: "2 4", label: "弱关系" };
        };
        return (
          <div style={{ maxWidth: 1180, margin: "0 auto" }}>
            <div style={{ display: "flex", alignItems: "flex-start", gap: 12, marginBottom: 14 }}>
              <div>
                <div style={{ fontSize: 15, fontWeight: 650 }}>👥 角色管理</div>
                <div style={{ fontSize: 11.5, color: T.text3, marginTop: 4 }}>
                  {chars.length} 个角色 · 立绘覆盖 {coverChars} 角 · {missingCover > 0 ? <span style={{ color: T.danger }}>{missingCover} 个缺立绘</span> : "立绘齐全"} · {relations.length} 条关系
                </div>
              </div>
              <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
                <button onClick={() => setCharTab(charTab === "cards" ? "relations" : "cards")} style={ghostBtn}>{charTab === "cards" ? "🕸 关系图谱" : "🃏 角色卡片"}</button>
                <button onClick={() => runCheck("characters")} title="检查角色与关系的语法 / 歧义 / 数值与类型矛盾（含本地规则）" style={ghostBtn}>🔍 检查</button>
                <button onClick={() => openCharForm(null)} style={accentBtn}>＋ 新建角色</button>
              </div>
            </div>
            <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap", marginBottom: 12 }}>
              <input value={charSearch} onChange={(e) => setCharSearch(e.target.value)} placeholder="搜索角色名、别名或身份" style={{ ...inputStyle, width: 240 }} />
              {roleOptions.map((r) => (
                <button key={r} onClick={() => setCharFilter(r)} style={{ height: 28, padding: "0 11px", borderRadius: 14, border: `1px solid ${charFilter === r ? "rgba(255,122,24,.42)" : T.line}`, background: charFilter === r ? T.accentSoft : "#191c22", color: charFilter === r ? T.accent : T.text2, cursor: "pointer", fontSize: 11.5, fontFamily: "inherit" }}>{r}{r === "全部" ? ` ${chars.length}` : ""}</button>
              ))}
            </div>
            {charTab === "cards" && (
              <>
                {charsLoading ? (
                  <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill,minmax(340px,1fr))", gap: 12 }}>
                    {[0, 1, 2].map((i) => (
                      <div key={i} style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, display: "grid", gridTemplateColumns: "104px 1fr", minHeight: 176, overflow: "hidden" }}>
                        <div style={{ background: "#22262f" }} />
                        <div style={{ padding: 12 }}>
                          <div style={{ height: 12, width: "42%", background: "#2a2f3a", borderRadius: 4, marginBottom: 10 }} />
                          <div style={{ height: 9, width: "78%", background: "#22262f", borderRadius: 4, marginBottom: 8 }} />
                          {[0, 1, 2, 3].map((j) => <div key={j} style={{ height: 6, background: "#1e222a", borderRadius: 3, marginBottom: 6 }} />)}
                        </div>
                      </div>
                    ))}
                  </div>
                ) : chars.length === 0 ? (
                  <div style={{ border: `1px dashed ${T.line}`, borderRadius: 12, padding: "56px 20px", textAlign: "center", color: T.text3, background: T.panel }}>
                    <div style={{ fontSize: 30, marginBottom: 8 }}>👥</div>
                    <b style={{ color: T.text1, fontSize: 14 }}>还没有角色</b>
                    <p style={{ fontSize: 12, color: T.text3, margin: "8px 0 14px", lineHeight: 1.7 }}>先建一个视点人物，故事才有落点；也可以先写剧情，再逐步补角色档案。</p>
                    <button onClick={() => openCharForm(null)} style={accentBtn}>＋ 新建角色</button>
                  </div>
                ) : (
                  <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill,minmax(340px,1fr))", gap: 12 }}>
                    {filteredChars.map((c) => {
                      const ps = portraitsOf(c.id);
                      const cover = ps.find((p) => p.path)?.path;
                      const roleColor = (c.role === "男主" || c.role === "女主" || c.role === "主角") ? T.accent : c.role === "对手" ? T.danger : c.role === "配角" ? "#5b8cff" : c.role === "背景" ? "#8f6bff" : T.text3;
                      return (
                        <div
                          key={c.id}
                          onClick={() => openCharForm(c)}
                          title="点击编辑该角色"
                          onMouseEnter={(e) => { e.currentTarget.style.borderColor = T.accent; }}
                          onMouseLeave={(e) => { e.currentTarget.style.borderColor = T.line; }}
                          style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, overflow: "hidden", display: "grid", gridTemplateColumns: "104px 1fr", minHeight: 178, cursor: "pointer", transition: "border-color .15s" }}
                        >
                          <div style={{ position: "relative", background: cover ? "#12141a" : "linear-gradient(160deg,#4a5f8a,#22293a)", overflow: "hidden" }}>
                            {cover ? <img src={convertFileSrc(cover)} alt="" style={{ width: "100%", height: "100%", objectFit: "cover" }} /> : <div style={{ height: "100%", display: "flex", alignItems: "center", justifyContent: "center", fontSize: 34, color: "rgba(255,255,255,.7)" }}>🧑</div>}
                            <span style={{ position: "absolute", top: 6, left: 6, fontSize: 9.5, padding: "2px 7px", borderRadius: 5, background: roleColor, color: "#fff", fontWeight: 600 }}>{c.role || "未定位"}</span>
                            <span style={{ position: "absolute", bottom: 0, left: 0, right: 0, fontSize: 9.5, padding: "3px 6px", background: "rgba(0,0,0,.58)", color: "#dfe3ec", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{c.current_status || "未出场"}</span>
                          </div>
                          <div style={{ padding: "10px 12px", display: "flex", flexDirection: "column", gap: 6, minWidth: 0 }}>
                            <div style={{ display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
                              <span style={{ fontWeight: 650, fontSize: 13.5 }}>{c.name}</span>
                              {c.alias && <span style={{ fontSize: 10, color: T.text3 }}>{c.alias}</span>}
                              {c.traits.split(/[,，、]/).filter(Boolean).slice(0, 2).map((t) => <span key={t} style={{ fontSize: 9.5, padding: "1px 6px", borderRadius: 8, background: T.accentSoft, color: T.accent }}>{t}</span>)}
                            </div>
                            <div style={{ fontSize: 10.5, color: T.text3, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{[c.one_liner || c.profile, [c.age, c.gender].filter(Boolean).join(" · "), c.faction].filter(Boolean).join(" · ") || "（暂无身份设定）"}</div>
                            <div style={{ display: "flex", flexDirection: "column", gap: 3 }}>
                              {([["体力", c.stat_stamina, "#f0554d,#ff9a45"], ["理智", c.stat_sanity, "#5b8cff,#8f6bff"], ["存在感", c.stat_screen, "#ff7a18,#ffb066"]] as [string, number, string][]).map(([lb, v, grad]) => (
                                <div key={lb} style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 9.5 }}>
                                  <span style={{ width: 22, color: T.text3 }}>{lb}</span>
                                  <span style={{ flex: 1, height: 5, borderRadius: 3, background: "#12141a", overflow: "hidden" }}><i style={{ display: "block", height: "100%", width: `${v}%`, background: `linear-gradient(90deg,${grad})` }} /></span>
                                  <span style={{ width: 20, textAlign: "right", color: T.text2, fontFamily: "monospace" }}>{v}</span>
                                </div>
                              ))}
                            </div>
                            <div style={{ display: "flex", alignItems: "center", gap: 8, marginTop: "auto", paddingTop: 4, borderTop: `1px solid ${T.lineSoft}`, fontSize: 10, color: T.text3 }}>
                              <span>📖 {nodesOf(c.id)} 节点</span>
                              <span>💬 {usageOf(c.id)} 句</span>
                              <span>🖼 {ps.length} 张</span>
                              <span style={{ marginLeft: "auto" }} />
                              <button onClick={(e) => { e.stopPropagation(); openCharForm(c); }} style={miniBtn}>编辑</button>
                              <button onClick={(e) => { e.stopPropagation(); removeChar(c.id, c.name); }} style={{ ...miniBtn, color: T.danger }}>删除</button>
                            </div>
                          </div>
                        </div>
                      );
                    })}
                    <div onClick={() => openCharForm(null)} style={{ border: `1px dashed ${T.line}`, borderRadius: 12, minHeight: 178, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 6, color: T.text3, cursor: "pointer", background: "transparent" }}>
                      <span style={{ fontSize: 24 }}>＋</span>
                      <b style={{ fontSize: 12.5, color: T.text2 }}>新建角色</b>
                      <small style={{ fontSize: 10 }}>手写档案，或让 AI 生成</small>
                    </div>
                  </div>
                )}
              </>
            )}
            {charTab === "relations" && (() => {
              const relRoot = (charGraphRootId != null ? chars.find((c) => c.id === charGraphRootId) : undefined)
                ?? chars.find((c) => c.role === "男主" || c.role === "女主" || c.role === "主角")
                ?? chars.slice().sort((a, b) => relCountOf(b.id) - relCountOf(a.id))[0]
                ?? null;
              const centerRels = relRoot ? relations.filter((r) => r.from_character_id === relRoot.id || r.to_character_id === relRoot.id) : [];
              const connectedIds = new Set<number>();
              centerRels.forEach((r) => { connectedIds.add(r.from_character_id); connectedIds.add(r.to_character_id); });
              const others = relRoot ? chars.filter((c) => c.id !== relRoot.id && connectedIds.has(c.id)) : [];
              const pos = new Map<number, { x: number; y: number }>();
              if (relRoot) pos.set(relRoot.id, { x: 400, y: 190 });
              others.forEach((c, i) => {
                const a = (2 * Math.PI * i) / Math.max(others.length, 1) - Math.PI / 2 + (others.length === 2 ? 0.55 : 0);
                pos.set(c.id, { x: 400 + 250 * Math.cos(a), y: 190 + 128 * Math.sin(a) });
              });
              return (
                <div style={{ display: "grid", gridTemplateColumns: "minmax(380px,1.25fr) minmax(310px,1fr)", gap: 14, alignItems: "start" }}>
                  <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: "#12141a", padding: 10 }}>
                    <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "2px 4px 8px", fontSize: 11.5, color: T.text3, flexWrap: "wrap" }}>
                      <b style={{ color: T.text2 }}>关系网络</b>
                      <span>{chars.length} 角色 · {centerRels.length} 条直接关系</span>
                      <span style={{ marginLeft: "auto", display: "flex", gap: 8, flexWrap: "wrap" }}>
                        <span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}><i style={{ width: 14, borderTop: "2.4px solid #ff9a45", display: "block" }} />强关联</span>
                        <span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}><i style={{ width: 14, borderTop: `2px solid ${T.danger}`, display: "block" }} />敌对</span>
                        <span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}><i style={{ width: 14, borderTop: `1.6px dashed ${T.ok}`, display: "block" }} />可信任</span>
                        <span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}><i style={{ width: 14, borderTop: "1.6px dashed #5b8cff", display: "block" }} />存疑</span>
                      </span>
                    </div>
                    <div style={{ display: "flex", alignItems: "center", gap: 4, padding: "0 4px 8px", flexWrap: "wrap" }}>
                      <span style={{ fontSize: 10.5, color: T.text3, marginRight: 2 }}>中心角色</span>
                      <span onClick={() => setCharGraphRootId(null)} style={{ fontSize: 10.5, padding: "2px 8px", borderRadius: 10, cursor: "pointer", border: `1px solid ${charGraphRootId == null ? T.accent : T.line}`, background: charGraphRootId == null ? T.accentSoft : "#191c22", color: charGraphRootId == null ? T.accent : T.text2, fontWeight: charGraphRootId == null ? 600 : 400 }}>自动</span>
                      {chars.map((c) => {
                        const active = relRoot?.id === c.id;
                        return (
                          <span key={c.id} onClick={() => setCharGraphRootId(c.id)} style={{ fontSize: 10.5, padding: "2px 8px", borderRadius: 10, cursor: "pointer", border: `1px solid ${active ? T.accent : T.line}`, background: active ? T.accentSoft : "#191c22", color: active ? T.accent : T.text2, fontWeight: active ? 600 : 400 }}>{c.name}</span>
                        );
                      })}
                    </div>
                    {chars.length === 0 ? (
                      <div style={{ padding: "70px 20px", textAlign: "center", color: T.text3, fontSize: 12.5 }}>还没有角色，先切到「角色卡片」新建角色。</div>
                    ) : centerRels.length === 0 ? (
                      <div style={{ padding: "70px 20px", textAlign: "center", color: T.text3, fontSize: 12.5 }}>「{relRoot?.name ?? "该角色"}」暂无直接关系，可在右侧新增。</div>
                    ) : (
                      <svg viewBox="0 0 800 380" style={{ width: "100%", height: 380, display: "block" }}>
                        <defs>
                          <marker id="relArrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto" markerUnits="strokeWidth"><path d="M0,0 L8,4 L0,8 Z" fill="#ff9a45" /></marker>
                        </defs>
                        {centerRels.map((r) => {
                          const p1 = pos.get(r.from_character_id);
                          const p2 = pos.get(r.to_character_id);
                          if (!p1 || !p2) return null;
                          const nature = relNature(r);
                          const dx = p2.x - p1.x, dy = p2.y - p1.y;
                          const len = Math.sqrt(dx * dx + dy * dy) || 1;
                          const sx = p1.x + (dx / len) * (relRoot && r.from_character_id === relRoot.id ? 26 : 20);
                          const sy = p1.y + (dy / len) * (relRoot && r.from_character_id === relRoot.id ? 26 : 20);
                          const ex = p2.x - (dx / len) * (relRoot && r.to_character_id === relRoot.id ? 26 : 20);
                          const ey = p2.y - (dy / len) * (relRoot && r.to_character_id === relRoot.id ? 26 : 20);
                          return (
                            <g key={r.id}>
                              <line x1={sx} y1={sy} x2={ex} y2={ey} stroke={nature.color} strokeWidth={nature.label === "强关联" ? 2.4 : 1.7} strokeDasharray={nature.dash || undefined} markerEnd="url(#relArrow)" opacity={0.9} />
                              <text x={(sx + ex) / 2} y={(sy + ey) / 2 - 5} textAnchor="middle" fontSize="10" fill="#fff" fontWeight={600} paintOrder="stroke" stroke="#12141a" strokeWidth={3}>{(r.relation_type || "陌生人")}{r.strength ? ` ${r.strength}` : ""}</text>
                            </g>
                          );
                        })}
                        {chars.map((c) => {
                          const p = pos.get(c.id);
                          if (!p) return null;
                          const isRoot = relRoot?.id === c.id;
                          const emo = emotionInfo(c.current_status || "");
                          const dx = p.x - 400, dy = p.y - 190;
                          const dlen = Math.sqrt(dx * dx + dy * dy) || 1;
                          const r = isRoot ? 25 : 18;
                          const roleOutX = (dx / dlen) * (r + 12);
                          const roleOutY = (dy / dlen) * (r + 12);
                          const roleAnchor = dx > 10 ? "start" : dx < -10 ? "end" : "middle";
                          return (
                            <g key={c.id}>
                              <circle cx={p.x} cy={p.y} r={r} fill={isRoot ? T.accent : T.panel3} stroke={isRoot ? T.accent : (emo.label ? emo.color : T.line)} strokeWidth={isRoot ? 2 : 1.6} />
                              <text x={p.x} y={isRoot && c.role ? p.y - 2 : p.y + 3.5} textAnchor="middle" fontSize={isRoot ? 11.5 : 11} fill={isRoot ? "#231505" : T.text1} fontWeight={isRoot ? 700 : 600}>{c.name}</text>
                              {c.role && (isRoot ? (
                                <text x={p.x} y={p.y + 10} textAnchor="middle" fontSize="8.5" fill="#231505" fontWeight={600}>{c.role}</text>
                              ) : (
                                <text x={p.x + roleOutX} y={p.y + roleOutY} textAnchor={roleAnchor} fontSize="9" fill={T.text3} paintOrder="stroke" stroke="#12141a" strokeWidth={3}>{c.role}</text>
                              ))}
                            </g>
                          );
                        })}
                      </svg>
                    )}
                  </div>
                  <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
                    {chars.length >= 2 && (
                      <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, padding: 12 }}>
                        <div style={{ fontSize: 12.5, fontWeight: 650, marginBottom: 8 }}>{relEditingId ? "编辑关系" : "新增关系"}</div>
                        <div style={{ display: "grid", gridTemplateColumns: "1fr 18px 1fr", gap: 6, alignItems: "center" }}>
                          <select value={rFrom} onChange={(e) => setRFrom(Number(e.target.value))} style={{ ...inputStyle, padding: "6px 8px" }}>
                            <option value={0}>— 从 —</option>
                            {chars.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
                          </select>
                          <span style={{ textAlign: "center", color: T.text3 }}>{rDir === "双向" ? "⟷" : "→"}</span>
                          <select value={rTo} onChange={(e) => setRTo(Number(e.target.value))} style={{ ...inputStyle, padding: "6px 8px" }}>
                            <option value={0}>— 到 —</option>
                            {chars.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
                          </select>
                        </div>
                        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 6, marginTop: 6 }}>
                          <input value={rType} onChange={(e) => setRType(e.target.value)} placeholder="关系类型（父女 / 兄妹 / 敌对 / 同学）" style={{ ...inputStyle, padding: "6px 8px" }} />
                          <select value={rDir} onChange={(e) => setRDir(e.target.value as "单向" | "双向")} style={{ ...inputStyle, padding: "6px 8px" }}>
                            <option value="单向">单向</option>
                            <option value="双向">双向</option>
                          </select>
                        </div>
                        <div style={{ display: "grid", gridTemplateColumns: "1fr 90px", gap: 8, alignItems: "center", marginTop: 6 }}>
                          <input type="range" min={0} max={100} value={rStrength} onChange={(e) => setRStrength(Number(e.target.value))} />
                          <span style={{ fontSize: 11, color: T.text2, textAlign: "right" }}>强度 {rStrength}</span>
                        </div>
                        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 6, marginTop: 6 }}>
                          <select value={rState} onChange={(e) => setRState(e.target.value)} title={REL_STATE_HINT[rState] || ""} style={{ ...inputStyle, padding: "6px 8px" }}>
                            {REL_STATES.map((s) => <option key={s} value={s}>状态：{s}</option>)}
                          </select>
                          <input value={rEvidence} onChange={(e) => setREvidence(e.target.value)} placeholder="出处 / 证据" style={{ ...inputStyle, padding: "6px 8px" }} />
                        </div>
                        <div style={{ fontSize: 10.5, color: T.text3, marginTop: 4 }}>{REL_STATE_HINT[rState] || ""}</div>
                        <textarea value={rDescription} onChange={(e) => setRDescription(e.target.value)} rows={2} placeholder="关系描述（会注入 AI 的关系上下文）" style={{ ...inputStyle, marginTop: 6, resize: "vertical", lineHeight: 1.6 }} />
                        <div style={{ display: "flex", gap: 8, marginTop: 8 }}>
                          <button onClick={saveRelation} style={accentBtn}>{relEditingId ? "保存修改" : "＋ 添加关系"}</button>
                          {relEditingId && <button onClick={resetRelationForm} style={ghostBtn}>取消编辑</button>}
                        </div>
                      </div>
                    )}
                    <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, overflow: "hidden" }}>
                      <div style={{ padding: "10px 12px", background: T.panel2, borderBottom: `1px solid ${T.lineSoft}`, fontSize: 11.5, color: T.text2 }}>关系条目 · {relations.length} 条</div>
                      <div style={{ maxHeight: 420, overflowY: "auto" }}>
                        {relations.length === 0 && <div style={{ padding: "26px 14px", textAlign: "center", color: T.text3, fontSize: 12 }}>还没有关系。</div>}
                        {relations.map((r) => (
                          <div key={r.id} style={{ padding: "10px 12px", borderBottom: `1px solid ${T.lineSoft}`, display: "flex", flexDirection: "column", gap: 6 }}>
                            <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 12.5, fontWeight: 600 }}>
                              <span>{charNameOf(chars, r.from_character_id)}</span>
                              <span style={{ color: T.text3 }}>{r.direction === "双向" ? "⟷" : "→"}</span>
                              <span>{charNameOf(chars, r.to_character_id)}</span>
                              <span style={{ marginLeft: "auto", fontSize: 9.5, padding: "1px 7px", borderRadius: 8, border: `1px solid ${T.line}`, color: T.text3 }}>{r.relation_type || "陌生人"} · {r.strength} · {normRelState(r.state)}</span>
                            </div>
                            {r.direction === "双向" && r.split_matched && r.split_records.length > 0 && (
                              <button onClick={() => setRelExpandId(relExpandId === r.id ? null : r.id)} style={{ alignSelf: "flex-start", height: 22, padding: "0 8px", borderRadius: 6, border: `1px solid ${T.line}`, background: "#191c22", color: T.text3, cursor: "pointer", fontSize: 10, fontFamily: "inherit" }}>
                                {relExpandId === r.id ? "收起自动拆分" : `展开自动拆分 ${r.split_records.length} 条`}
                              </button>
                            )}
                            {relExpandId === r.id && r.split_records.map((s, i) => (
                              <div key={`${r.id}-split-${i}`} style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 11, color: T.text2, background: "#191c22", border: `1px solid ${T.lineSoft}`, borderRadius: 7, padding: "5px 8px" }}>
                                <span style={{ fontSize: 9.5, padding: "1px 6px", borderRadius: 6, background: T.accentSoft, color: T.accent }}>单向</span>
                                <span>{s.text}</span>
                              </div>
                            ))}
                            {r.description && <div style={{ fontSize: 11, color: T.text2, lineHeight: 1.65 }}>{r.description}</div>}
                            {r.evidence && <div style={{ fontSize: 10.5, color: T.text3 }}>出处：{r.evidence}</div>}
                            <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 10, color: T.text3 }}>
                              <span title={REL_STATE_HINT[normRelState(r.state)] || ""} style={{ padding: "1px 6px", borderRadius: 7, background: "#191c22", border: `1px solid ${T.lineSoft}` }}>{normRelState(r.state)}</span>
                              <span style={{ marginLeft: "auto" }} />
                              <button onClick={() => editRelation(r)} style={miniBtn}>编辑</button>
                              <button onClick={() => removeRelation(r.id)} style={{ ...miniBtn, color: T.danger }}>删除</button>
                            </div>
                          </div>
                        ))}
                      </div>
                    </div>
                  </div>
                </div>
              );
            })()}
            {charFormOpen && (
              <div onClick={() => setCharFormOpen(false)} style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,.56)", zIndex: 60, display: "flex", alignItems: "flex-start", justifyContent: "center", padding: "40px 20px", overflowY: "auto" }}>
                <div onClick={(e) => e.stopPropagation()} style={{ width: 880, maxWidth: "100%", background: T.panel, border: `1px solid ${T.line}`, borderRadius: 14, boxShadow: "0 24px 70px rgba(0,0,0,.55)", overflow: "hidden" }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "13px 16px", borderBottom: `1px solid ${T.lineSoft}` }}>
                    <b style={{ fontSize: 14 }}>{charEditingId ? "编辑角色" : "新建角色"}</b>
                    <span style={{ fontSize: 11.5, color: T.text3 }}>{charEditingId ? "保存后版本 +1" : "左侧档案 + 右侧立绘预览"}</span>
                    <button onClick={() => setCharFormOpen(false)} style={{ marginLeft: "auto", background: "none", border: "none", color: T.text3, cursor: "pointer", fontSize: 16, fontFamily: "inherit" }}>✕</button>
                  </div>
                  <div style={{ display: "grid", gridTemplateColumns: "1fr 220px", gap: 16, padding: 16 }}>
                    <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
                      <div style={{ display: "flex", gap: 10 }}>
                        <label style={labelStyle}>姓名 *<input value={charForm.name} onChange={(e) => setCharForm({ ...charForm, name: e.target.value })} style={inputStyle} /></label>
                        <label style={labelStyle}>别名 / 称呼<input value={charForm.alias} onChange={(e) => setCharForm({ ...charForm, alias: e.target.value })} style={inputStyle} /></label>
                        <label style={{ ...labelStyle, flex: "0 0 110px" }}>定位<select value={charForm.role} onChange={(e) => setCharForm({ ...charForm, role: e.target.value })} style={inputStyle}>{["未定位", "男主", "女主", "对手", "配角", "背景", "其他", "主角"].map((r) => <option key={r} value={r}>{r}</option>)}</select></label>
                      </div>
                      <div style={{ display: "flex", gap: 10 }}>
                        <label style={labelStyle}>阵营 / 归属势力<input value={charForm.faction} onChange={(e) => setCharForm({ ...charForm, faction: e.target.value })} style={inputStyle} /></label>
                        <label style={labelStyle}>年龄<input value={charForm.age} onChange={(e) => setCharForm({ ...charForm, age: e.target.value })} style={inputStyle} /></label>
                        <label style={labelStyle}><span>性别 <span style={{ color: T.danger }}>*</span></span><input value={charForm.gender} onChange={(e) => setCharForm({ ...charForm, gender: e.target.value })} style={inputStyle} /></label>
                      </div>
                      <label style={labelStyle}>首次出场<input value={charForm.first_appearance} onChange={(e) => setCharForm({ ...charForm, first_appearance: e.target.value })} placeholder="第 1 章 第 1 节" style={inputStyle} /></label>
                      <label style={labelStyle}>一句话身份<input value={charForm.one_liner} onChange={(e) => setCharForm({ ...charForm, one_liner: e.target.value })} style={inputStyle} /></label>
                      <label style={labelStyle}>人物小传<textarea value={charForm.profile} onChange={(e) => setCharForm({ ...charForm, profile: e.target.value })} rows={3} style={{ ...inputStyle, resize: "vertical", lineHeight: 1.65 }} /></label>
                      <label style={labelStyle}>说话方式约束（注入 AI 生成）<textarea value={charForm.speech_style} onChange={(e) => setCharForm({ ...charForm, speech_style: e.target.value })} rows={2} style={{ ...inputStyle, resize: "vertical", lineHeight: 1.65 }} /></label>
                      <label style={labelStyle}>性格标签（逗号分隔）<input value={charForm.traits} onChange={(e) => setCharForm({ ...charForm, traits: e.target.value })} placeholder="克制，偏执，责任感过重" style={inputStyle} /></label>
                      {/* v3.4「角色卡 v2」：作者设定（进 system 提示词的角色段） */}
                      <div style={{ borderTop: `1px dashed ${T.line}`, paddingTop: 10, marginTop: 2, fontSize: 11, color: T.text3, lineHeight: 1.6 }}>
                        以下五项（外貌 + 过去/动机/目的/准则）会注入 AI 的角色上下文（稳定区，几乎不变）。留空则不出现。
                      </div>
                      <label style={labelStyle}>外貌（他长什么样）
                        <textarea value={charForm.appearance} onChange={(e) => setCharForm({ ...charForm, appearance: e.target.value })} rows={2} placeholder="例：瘦高，左手腕有一道旧疤，常年穿深灰外套" style={{ ...inputStyle, resize: "vertical" }} />
                      </label>
                      <label style={labelStyle}>过去（他的往事）<textarea value={charForm.background_past} onChange={(e) => setCharForm({ ...charForm, background_past: e.target.value })} rows={3} placeholder="例：幼年在修女院长大，十六岁那年一场大火带走了所有人。" style={{ ...inputStyle, resize: "vertical", lineHeight: 1.65 }} /></label>
                      <label style={labelStyle}>动机（他为什么这么做）<textarea value={charForm.motivation} onChange={(e) => setCharForm({ ...charForm, motivation: e.target.value })} rows={2} placeholder="例：想证明自己不是被丢下的那个。" style={{ ...inputStyle, resize: "vertical", lineHeight: 1.65 }} /></label>
                      <label style={labelStyle}>目的（他想达成什么）<textarea value={charForm.goal} onChange={(e) => setCharForm({ ...charForm, goal: e.target.value })} rows={2} placeholder="例：查清那场火是谁放的。" style={{ ...inputStyle, resize: "vertical", lineHeight: 1.65 }} /></label>
                      <label style={labelStyle}>行为准则（他会怎么做 / 绝不做什么）<textarea value={charForm.behavior_rules} onChange={(e) => setCharForm({ ...charForm, behavior_rules: e.target.value })} rows={2} placeholder="例：绝不先开口道歉；被逼急了会突然笑。" style={{ ...inputStyle, resize: "vertical", lineHeight: 1.65 }} /></label>
                      <label style={labelStyle}>对话示例（few-shot，仅焦点角色进提示词）
                        <textarea value={charForm.dialogue_examples} onChange={(e) => setCharForm({ ...charForm, dialogue_examples: e.target.value })} rows={4} placeholder={"例：\nA：你还在生气？\n她：没有。（把杯子放得比平时重一点）"} style={{ ...inputStyle, resize: "vertical", lineHeight: 1.65 }} />
                        <span style={{ fontSize: 10.5, color: T.text3, lineHeight: 1.5 }}>只用来教 AI 模仿语气与反应方式，不会被当成剧情续写；最多 800 字。</span>
                      </label>
                      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10 }}>
                        {([["体力", "stat_stamina"], ["理智", "stat_sanity"], ["存在感", "stat_screen"]] as [string, keyof CharacterForm][]).map(([lb, key]) => (
                          <label key={lb} style={{ ...labelStyle, flex: "none" }}>
                            <span>{lb} <b style={{ color: T.accent }}>{charForm[key] as number}</b></span>
                            <input type="range" min={0} max={100} value={charForm[key] as number} onChange={(e) => setCharForm((f) => ({ ...f, [key]: Number(e.target.value) } as CharacterForm))} />
                          </label>
                        ))}
                      </div>
                      {charError && (
                        <div style={{ border: `1px solid ${T.danger}55`, background: "rgba(240,85,77,.08)", borderRadius: 8, padding: "10px 12px", fontSize: 11.5, color: "#ff8c85", lineHeight: 1.7 }}>
                          ⚠ {charError}
                          <div style={{ display: "flex", gap: 8, marginTop: 8 }}>
                            <button onClick={saveCharForm} style={miniBtn}>重试保存</button>
                            <button onClick={() => setCharError("")} style={miniBtn}>手动补全</button>
                          </div>
                        </div>
                      )}
                    </div>
                    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                      <div style={{ border: `1px dashed ${T.line}`, borderRadius: 12, minHeight: 230, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", color: T.text3, background: "#191c22", overflow: "hidden" }}>
                        {charEditingId && portraitsOf(charEditingId)[0]?.path ? (
                          <img src={convertFileSrc(portraitsOf(charEditingId)[0].path as string)} alt="" style={{ width: "100%", height: 230, objectFit: "cover" }} />
                        ) : (
                          <>
                            <span style={{ fontSize: 34 }}>🖼</span>
                            <span style={{ fontSize: 11.5, marginTop: 6 }}>立绘预览</span>
                            <small style={{ fontSize: 9.5 }}>未绑定</small>
                          </>
                        )}
                      </div>
                      <button onClick={() => { setCharFormOpen(false); setView("portrait"); }} style={ghostBtn}>📤 从素材库选择</button>
                      <button onClick={randomCharForm} style={ghostBtn}>🎲 随机灵感</button>
                      <button onClick={saveCharForm} style={{ ...accentBtn, width: "100%" }}>💾 保存角色</button>
                      <div style={{ fontSize: 10, color: T.text3, lineHeight: 1.7 }}>保存后写入角色档案；说话方式、身份与小传会注入剧情 AI 的角色上下文。</div>
                    </div>
                  </div>
                </div>
              </div>
            )}
          </div>
        );
      })()
  );
}
