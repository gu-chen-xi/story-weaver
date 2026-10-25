// 「🎨 立绘素材」页（素材库主页 + 详情页 + 4 个弹窗）。
//
// 拆分说明（2026-10-17，第一步）：从 App.tsx 把**视图**整体搬到这里。
// ⚠ 过渡状态：61 个 props —— 26 个 state 与 12 个处理函数仍留在 App。
//   下一步会把 state + 处理函数一起搬进来，props 会缩到个位数。
//   先分两步的理由：一次性搬 state 要同时改 App 里其它 40+ 处引用，风险远高于收益。

import { useEffect, useRef, useState, type Dispatch, type SetStateAction } from "react";
import type { AppView } from "../types";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { PORTRAIT_GRADS, EXPRESSION_PRESETS } from "../constants";
import { DEFAULT_NEGATIVE } from "../plugins";
import type { PortraitItem } from "../types";

import { T } from "../theme";
import type { Story } from "../types";
import { useConfirm } from "../confirm";
import { customGroupsOf, groupBuckets, groupsOf } from "../lib/portraitGroups";
import { expressionTagOf, isSystemTag, labelOfVariant, variantSlots } from "../lib/portraitVariants";

/// 顶部导航的视图名（与 App 的 view state 保持一致）
export type ViewName = AppView;

export type PortraitPageProps = {
  pDetail: number | null;
  setPDetail: Dispatch<SetStateAction<number | null>>;
  portraits: PortraitItem[];
  loadPortraits: () => Promise<void>;
  setMsg: Dispatch<SetStateAction<string>>;
  stories: Story[];
  /// 打开「立绘生成」面板（在 App 里，会切到插件页并预填提示词）
  openGenPanel: (prefill?: {
    prompt?: string; negative?: string; width?: number; height?: number;
    seed?: string; steps?: string; cfg?: string;
    characterId?: number | null; basePortraitId?: number | null; note?: string;
  }) => void;
};

/** 预览缩放范围：1×（适配面板）~ 4× —— 再大就是马赛克了 */
const clampZoom = (z: number) => Math.min(4, Math.max(1, Number.isFinite(z) ? z : 1));

export function PortraitPage(props: PortraitPageProps) {
  const confirm = useConfirm();
  const { pDetail, setPDetail, portraits, loadPortraits, setMsg, stories, openGenPanel } = props;
  // ===== 以下为从 App.tsx 搬进来的立绘状态与处理函数（2026-10-17）=====
  // 导出合规确认：每次都提示（刻意不做"不再提醒" —— 这是一种不该被长期关掉的合规提醒）
  const exportAckRemember = false;
  const [pView, setPView] = useState<"grid" | "masonry" | "group">("grid");   // 立绘素材视图
  const [pSel, setPSel] = useState<Set<number>>(new Set());
  const [pSearch, setPSearch] = useState("");
  const [pRole, setPRole] = useState("全部");
  const [pSource, setPSource] = useState<"全部" | "手工" | "AI 生成">("全部");   // 按来源筛选（合规：一眼看清哪些是 AI 生成）
  const [pSort, setPSort] = useState<"date" | "name">("date");
  const [pModal, setPModal] = useState<"import" | "create" | null>(null);
  const [pDraft, setPDraft] = useState({ srcPath: "", name: "", role: "", ver: "v1", tags: "", ratio: "", group: "" });
  const doPick = async (preRole = "", preGroup = "") => {
    try {
      const sel = await open({ multiple: false, filters: [{ name: "图片", extensions: ["png", "jpg", "jpeg", "webp", "bmp", "gif"] }] });
      if (!sel) return;
      const p = Array.isArray(sel) ? sel[0] : sel;
      const base = p.split(/[\\/]/).pop() || "素材";
      const nm = base.replace(/\.[^.]+$/, "");
      setPDraft({ srcPath: p, name: nm, role: preRole, ver: "v1", tags: "", ratio: "", group: preGroup });
      setPModal("import");
    } catch (e) { setMsg("❌ 打开文件对话框失败: " + String(e)); }
  };
  const submitPModal = async () => {
    if (!pDraft.name.trim()) { setMsg("⚠️ 素材名称不能为空"); return; }
    try {
      if (pModal === "import") {
        await invoke("portrait_import", { srcPath: pDraft.srcPath, name: pDraft.name.trim(), role: "", ver: "v1", tags: pDraft.tags.trim(), group: pDraft.group.trim() });
        setMsg("✅ 已导入立绘素材");
      } else {
        await invoke("portrait_create", { name: pDraft.name.trim(), role: "", ver: "v1", ratio: pDraft.ratio.trim(), tags: pDraft.tags.trim() });
        setMsg("✅ 已新建素材（无图，稍后可导入图片）");
      }
      setPModal(null);
      await loadPortraits();
    } catch (e) { setMsg("❌ " + String(e)); }
  };
  const [pBindOpen, setPBindOpen] = useState(false);
  const [bindStory, setBindStory] = useState(0);
  const [bindChar, setBindChar] = useState(0);
  const [bindChars, setBindChars] = useState<{ id: number; name: string }[]>([]);
  const loadBindChars = async (sid: number) => {
    if (!sid) { setBindChars([]); return; }
    try { setBindChars(await invoke<{ id: number; name: string }[]>("list_characters", { storyId: String(sid) })); } catch (e) { setBindChars([]); }
  };
  const doBind = async () => {
    if (!pDetail) return;
    if (!bindStory || !bindChar) { setMsg("⚠️ 请选择故事和角色"); return; }
    try {
      await invoke("portrait_bind", { id: pDetail, storyId: bindStory, characterId: bindChar });
      setMsg("✅ 已绑定角色");
      setPBindOpen(false);
      await loadPortraits();
    } catch (e) { setMsg("❌ " + String(e)); }
  };
  const doUnbind = async () => {
    if (!pDetail) return;
    try {
      await invoke("portrait_bind", { id: pDetail, storyId: null, characterId: null });
      setMsg("🟡 已解除绑定");
      setPBindOpen(false);
      await loadPortraits();
    } catch (e) { setMsg("❌ " + String(e)); }
  };
  const [pGroupOpen, setPGroupOpen] = useState(false);
  const [pGroupInput, setPGroupInput] = useState("");
  const [pTagOpen, setPTagOpen] = useState(false);
  const [pTagInput, setPTagInput] = useState("");
  const [pDelOpen, setPDelOpen] = useState(false);
  const doGroupSet = async () => {
    if (!pGroupInput.trim()) { setMsg("⚠️ 请输入或选择分组名"); return; }
    try {
      const n = await invoke<number>("portrait_group_set", { ids: Array.from(pSel), group: pGroupInput.trim() });
      setMsg(`✅ 已把 ${n} 个素材加入分组「${pGroupInput.trim()}」（原分组保留）`);
      setPGroupOpen(false); setPGroupInput(""); setPSel(new Set());
      await loadPortraits();
    } catch (e) { setMsg("❌ " + String(e)); }
  };
  const doTagBatch = async () => {
    if (!pTagInput.trim()) { setMsg("⚠️ 请输入标签"); return; }
    try {
      const n = await invoke<number>("portrait_tag_batch", { ids: Array.from(pSel), tags: pTagInput.trim() });
      setMsg(`✅ 已为 ${n} 个素材追加标签`);
      setPTagOpen(false); setPTagInput(""); setPSel(new Set());
      await loadPortraits();
    } catch (e) { setMsg("❌ " + String(e)); }
  };
  const doPExport = async () => {
    const ids = Array.from(pSel);
    // 合规确认：只对**含 AI 生成**的导出弹一次（手工素材不打扰）
    const aiCount = portraits.filter((x) => ids.includes(x.id) && (x.source ?? "").startsWith("ai-")).length;
    if (aiCount > 0 && !exportAckRemember) {
      const proceed = await confirm({
        title: "导出含 AI 生成的素材",
        body:
          `本次导出的 ${ids.length} 个素材里有 ${aiCount} 个是「AI 生成」内容。\n\n` +
          `· 导出文件名会带 _AI 后缀，并附「导出清单.json」（含提示词与参数）\n` +
          `· 对外发布时请显式标注「AI 生成」，且不要删除图上的 AI 标识\n` +
          `  （《人工智能生成合成内容标识办法》2025-09-01 施行）\n\n` +
          `（确认后可在「合规与责任」页随时复核）`,
        okText: "导出",
      });
      if (!proceed) return;
    }
    try {
      setMsg(await invoke<string>("portrait_export", { ids }));
    } catch (e) { setMsg("❌ " + String(e)); }
  };
  const doDelete = async () => {
    try {
      const n = await invoke<number>("portrait_delete", { ids: Array.from(pSel) });
      setMsg(`🗑 已删除 ${n} 个素材`);
      setPDelOpen(false); setPSel(new Set());
      await loadPortraits();
    } catch (e) { setMsg("❌ " + String(e)); }
  };
  // （「改角色」弹窗 2026-10-25 已由「表情标签」取代：角色归属统一看「绑定角色」）

  // ===== 「表情标签」（2026-10-25）=====
  // 一张立绘一个表情名，存在 tags 里（系统标签如「AI生成」保留不动）。
  // 切换表情差分时，监视器就是按这个标签显示 `1/3 冷静`。
  const [pExprOpen, setPExprOpen] = useState(false);
  const [pExprVal, setPExprVal] = useState("");
  // 预览缩放 + 拖拽（2026-10-25）：原来那 4 个按钮是假的，现在只留缩放与拖动且真能用
  const [zoom, setZoom] = useState(1);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const [dragging, setDragging] = useState(false);
  const dragRef = useRef<{ x: number; y: number; ox: number; oy: number } | null>(null);
  const frameRef = useRef<HTMLDivElement | null>(null);
  const imgRef = useRef<HTMLImageElement | null>(null);

  // 滚轮缩放：必须用**原生非 passive 监听**，否则 preventDefault 无效、页面会跟着滚（React 的 onWheel 是 passive）
  useEffect(() => {
    const el = imgRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      setZoom((z) => clampZoom(z - e.deltaY * 0.0012));
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [pDetail]);

  /** 把平移量夹在"放大后多出来的那部分"以内，避免把图拖飞出去只剩背景 */
  const clampOffset = (o: { x: number; y: number }, z: number) => {
    const el = frameRef.current;
    if (!el || z <= 1) return { x: 0, y: 0 };
    const maxX = (el.clientWidth * (z - 1)) / 2;
    const maxY = (el.clientHeight * (z - 1)) / 2;
    return { x: Math.max(-maxX, Math.min(maxX, o.x)), y: Math.max(-maxY, Math.min(maxY, o.y)) };
  };

  // 换素材：缩放与位置都回到初始
  useEffect(() => { setZoom(1); setOffset({ x: 0, y: 0 }); }, [pDetail]);
  // 缩放变化后重新夹一次（缩小回去时位移可能已经越界）
  useEffect(() => { setOffset((o) => clampOffset(o, zoom)); }, [zoom]);
  const doExprSave = async () => {
    if (!pDetail) return;
    const m = portraits.find((x) => x.id === pDetail);
    if (!m) return;
    try {
      const keep = m.tags.filter((t) => isSystemTag(t));
      const expr = pExprVal.trim();
      await invoke("portrait_tag_set", { id: pDetail, tags: [...keep, ...(expr ? [expr] : [])].join(",") });
      setMsg(expr ? `✅ 表情标签：${expr}` : "✅ 已清除表情标签");
      setPExprOpen(false);
      await loadPortraits();
    } catch (e) { setMsg("❌ " + String(e)); }
  };

  /** 把这张移出某个**自定义**分组（角色组由绑定关系派生，解绑才消失） */
  const removeFromGroup = async (id: number, group: string) => {
    try {
      await invoke<number>("portrait_group_remove", { ids: [id], group });
      setMsg(`🗂 已移出分组「${group}」`);
      await loadPortraits();
    } catch (e) { setMsg("❌ 移出分组失败：" + String(e)); }
  };

  /** 删除**当前这张**素材（记录 + 图片文件），详情面板里的入口 */
  const doDeleteOne = async (id: number) => {
    const ok = await confirm({
      title: "删除素材",
      body: "删除这个立绘素材？\n\n· 素材记录与图片文件都会删掉\n· 绑定关系一起解除\n· 不可撤销",
      danger: true,
    });
    if (!ok) return;
    try {
      await invoke<number>("portrait_delete", { ids: [id] });
      setMsg("🗑 已删除该素材");
      setPDetail(null);
      setPSel((prev) => { const n = new Set(prev); n.delete(id); return n; });
      await loadPortraits();
    } catch (e) { setMsg("❌ 删除失败：" + String(e)); }
  };
  const [pGenEditOpen, setPGenEditOpen] = useState(false);
  const [pGenForm, setPGenForm] = useState({ prompt: "", negative: "", seed: "", steps: "", cfg: "", model: "" });
  const doGenSave = async () => {
    if (!pDetail) return;
    try {
      await invoke("portrait_update_params", { id: pDetail, prompt: pGenForm.prompt, negative: pGenForm.negative, seed: pGenForm.seed, steps: pGenForm.steps, cfg: pGenForm.cfg, model: pGenForm.model });
      setMsg("✅ 生成参数已更新");
      setPGenEditOpen(false);
      await loadPortraits();
    } catch (e) { setMsg("❌ " + String(e)); }
  };
  const regenFromPortrait = (item: PortraitItem) => {
    openGenPanel({
      prompt: item.prompt || "",
      negative: item.negative || DEFAULT_NEGATIVE,
      width: item.width || 768,
      height: item.height || 1024,
      seed: item.seed || "",
      steps: item.steps || "",
      cfg: item.cfg || "",
      characterId: item.character_id ?? null,
      basePortraitId: item.id,
      note: `重出「${item.name}」：已带原提示词${item.seed ? `与种子 ${item.seed}` : "（原为随机种子，可填一个固定风格）"}；新图会记到它的重出链上`,
    });
  };

  return (
      (() => {
        // 角色筛选按**绑定角色**来（2026-10-25：原来按 role 字段，那字段已经不再编辑）
        const roles = ["全部", ...Array.from(new Set(portraits.map((m) => m.bound_character).filter((x): x is string => !!x)))];
        const totalMB = portraits.reduce((a, m) => a + m.size_bytes, 0);
        const fmtTotal = totalMB >= 1048576 ? (totalMB / 1048576).toFixed(1) + " MB" : totalMB >= 1024 ? Math.round(totalMB / 1024) + " KB" : totalMB + " B";
        const recent = portraits.length ? portraits.map((m) => m.date).sort().slice(-1)[0] : "—";
        const filtered = portraits
          .filter((m) => (pRole === "全部" || m.bound_character === pRole)
            && (pSource === "全部"
              || (pSource === "AI 生成" && (m.source ?? "").startsWith("ai-"))
              || (pSource === "手工" && !(m.source ?? "").startsWith("ai-")))
            && (!pSearch.trim() || (m.name + (m.bound_character ?? "") + m.tags.join("")).includes(pSearch.trim())))
          .sort((a, b) => pSort === "name" ? a.name.localeCompare(b.name, "zh") : (b.date + String(b.id)).localeCompare(a.date + String(a.id)));
        const gtag = (t: string) => t === "当前场景" || t === "待审核" ? { background: T.accentSoft, color: T.accent }
          : t === "冷色调" || t === "表情差分 ×4" || t === "年代差异" ? { background: "rgba(91,140,255,.14)", color: "#8ab4ff" }
          : t === "已定稿" ? { background: "rgba(55,201,120,.14)", color: "#5fdc9c" }
          : { background: T.panel3, color: T.text2 };
        const selSet = pSel;
        const toggleSel = (id: number) => setPSel((prev) => { const nx = new Set(prev); if (nx.has(id)) nx.delete(id); else nx.add(id); return nx; });
        const chipBtn = (active: boolean) => ({ display: "inline-flex", alignItems: "center", gap: 5, height: 28, padding: "0 11px", borderRadius: 14, border: `1px solid ${T.line}`, background: "#191c22", fontSize: 11.5, color: active ? T.accent : T.text2, cursor: "pointer", fontFamily: "inherit" });
        const fmtBtn = { display: "inline-flex", alignItems: "center", gap: 6, height: 32, padding: "0 14px", borderRadius: 8, fontSize: 12.5, cursor: "pointer", fontFamily: "inherit", border: `1px solid ${T.line}`, background: "#191c22", color: T.text2 };
        const bindModalJSX = pBindOpen && (
              <div style={{ position: "fixed", inset: 0, background: "rgba(8,10,14,.66)", zIndex: 121, display: "flex", alignItems: "center", justifyContent: "center" }} onClick={() => setPBindOpen(false)}>
                <div style={{ width: 430, maxWidth: "92vw", border: `1px solid #3a3f4b`, borderRadius: 14, background: T.panel, padding: 20, boxShadow: "0 30px 80px rgba(0,0,0,.6)" }} onClick={(e) => e.stopPropagation()}>
                  <div style={{ fontSize: 15, fontWeight: 650, marginBottom: 4 }}>🎯 绑定角色</div>
                  <div style={{ fontSize: 11.5, color: T.text3, marginBottom: 14 }}>将素材绑定到故事列表中的某个角色，方便后续按角色调用立绘。</div>
                  <div style={{ display: "grid", gridTemplateColumns: "72px 1fr", gap: "8px 12px", alignItems: "center" }}>
                    <label style={{ fontSize: 12, color: T.text2 }}>故事</label>
                    <select value={bindStory} onChange={(e) => { const v = Number(e.target.value); setBindStory(v); setBindChar(0); loadBindChars(v); }} style={{ padding: "7px 10px", background: "#191c22", color: T.text1, border: `1px solid ${T.line}`, borderRadius: 8, fontFamily: "inherit", fontSize: 12 }}>
                      <option value={0}>请选择故事…</option>
                      {stories.map((s) => <option key={s.id} value={s.id}>{s.title}</option>)}
                    </select>
                    <label style={{ fontSize: 12, color: T.text2 }}>角色</label>
                    <select value={bindChar} onChange={(e) => setBindChar(Number(e.target.value))} style={{ padding: "7px 10px", background: "#191c22", color: T.text1, border: `1px solid ${T.line}`, borderRadius: 8, fontFamily: "inherit", fontSize: 12 }}>
                      <option value={0}>{bindStory ? "请选择角色…" : "请先选择故事"}</option>
                      {bindChars.map((ch) => <option key={ch.id} value={ch.id}>{ch.name}</option>)}
                    </select>
                  </div>
                  <div style={{ display: "flex", gap: 10, marginTop: 16, justifyContent: "flex-end" }}>
                    <button onClick={doUnbind} style={{ height: 32, padding: "0 14px", borderRadius: 8, border: `1px solid ${T.warn}`, background: "transparent", color: T.warn, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" }}>解除绑定</button>
                    <button onClick={() => setPBindOpen(false)} style={{ height: 32, padding: "0 16px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" }}>取消</button>
                    <button onClick={doBind} style={{ height: 32, padding: "0 18px", borderRadius: 8, border: "none", background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505", fontWeight: 650, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" }}>确认绑定</button>
                  </div>
                </div>
              </div>
            );
        const exprJSX = pExprOpen && (() => {
          // 同一个角色下**已经被别的立绘占用**的表情名，这里置灰不许再选（用户 2026-10-25 要求）
          const cur = portraits.find((x) => x.id === pDetail) ?? null;
          const siblings = cur?.character_id
            ? portraits.filter((x) => x.character_id === cur.character_id && x.id !== cur.id)
            : [];
          const used = new Set(siblings.map((x) => expressionTagOf(x)).filter(Boolean));
          const pick = (t: string) => setPExprVal(t);
          return (
            <div style={{ position: "fixed", inset: 0, background: "rgba(8,10,14,.66)", zIndex: 125, display: "flex", alignItems: "center", justifyContent: "center" }} onClick={() => setPExprOpen(false)}>
              <div style={{ width: 460, maxWidth: "92vw", border: `1px solid #3a3f4b`, borderRadius: 14, background: T.panel, padding: 20, boxShadow: "0 30px 80px rgba(0,0,0,.6)" }} onClick={(e) => e.stopPropagation()}>
                <div style={{ fontSize: 15, fontWeight: 650, marginBottom: 4 }}>😀 表情标签</div>
                <div style={{ fontSize: 11.5, color: T.text3, marginBottom: 14, lineHeight: 1.7 }}>
                  一个立绘一个表情名，监视器的表情差分就用它当角标。<br />
                  {cur?.character_id
                    ? `同一个角色的其它立绘已经用掉的（${[...used].join(" / ") || "无"}）不能重复选。`
                    : "这张还没绑定角色，先绑定才能保证同角色下不重名。"}
                </div>
                <div style={{ display: "flex", flexWrap: "wrap", gap: 7, marginBottom: 12 }}>
                  {EXPRESSION_PRESETS.map((t) => {
                    const taken = used.has(t);
                    const on = pExprVal === t;
                    return (
                      <button key={t} disabled={taken} onClick={() => pick(t)}
                        title={taken ? "这个表情名已被同角色的另一张立绘占用" : undefined}
                        style={{
                          height: 28, padding: "0 12px", borderRadius: 14,
                          border: `1px solid ${on ? T.accent : T.line}`,
                          background: on ? T.accentSoft : "#191c22",
                          color: taken ? T.text3 : on ? T.accent : T.text2,
                          opacity: taken ? 0.45 : 1,
                          cursor: taken ? "not-allowed" : "pointer",
                          fontSize: 11.5, fontFamily: "inherit", textDecoration: taken ? "line-through" : "none",
                        }}>{t}</button>
                    );
                  })}
                </div>
                <input value={pExprVal} onChange={(e) => setPExprVal(e.target.value)}
                  placeholder="也可以自己填（如：回眸、落泪、怒视）"
                  style={{ width: "100%", padding: "7px 10px", boxSizing: "border-box" }} />
                <div style={{ display: "flex", gap: 10, marginTop: 16, justifyContent: "flex-end" }}>
                  <button onClick={() => setPExprVal("")} style={{ height: 32, marginRight: "auto", padding: "0 12px", borderRadius: 8, border: `1px solid ${T.line}`, background: "transparent", color: T.text3, cursor: "pointer", fontSize: 12, fontFamily: "inherit" }}>清除标签</button>
                  <button onClick={() => setPExprOpen(false)} style={{ height: 32, padding: "0 16px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" }}>取消</button>
                  <button onClick={doExprSave} style={{ height: 32, padding: "0 18px", borderRadius: 8, border: "none", background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505", fontWeight: 650, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" }}>保存</button>
                </div>
              </div>
            </div>
          );
        })();
        const pGenEditJSX = pGenEditOpen && (
              <div style={{ position: "fixed", inset: 0, background: "rgba(8,10,14,.66)", zIndex: 125, display: "flex", alignItems: "center", justifyContent: "center" }} onClick={() => setPGenEditOpen(false)}>
                <div style={{ width: 500, maxWidth: "92vw", border: `1px solid #3a3f4b`, borderRadius: 14, background: T.panel, padding: 20, boxShadow: "0 30px 80px rgba(0,0,0,.6)" }} onClick={(e) => e.stopPropagation()}>
                  <div style={{ fontSize: 15, fontWeight: 650, marginBottom: 4 }}>⌨️ 编辑生成参数</div>
                  <div style={{ fontSize: 11.5, color: T.text3, marginBottom: 14 }}>记录该立绘生成时的配方：提示词与关键参数（导入素材可手动填写，AI 生图后自动写入）。</div>
                  <div style={{ fontSize: 11, color: T.text3, marginBottom: 4 }}>提示词（正向描述）</div>
                  <textarea value={pGenForm.prompt} onChange={(e) => setPGenForm({ ...pGenForm, prompt: e.target.value })} rows={4} placeholder="生成该图时喂给模型的正向描述（可空）" style={{ width: "100%", boxSizing: "border-box", background: "#12141a", border: `1px solid ${T.line}`, borderRadius: 8, color: T.text1, fontSize: 12, padding: "8px 10px", fontFamily: "Consolas, 'Courier New', monospace", resize: "vertical", lineHeight: 1.6 }} />
                  <div style={{ fontSize: 11, color: T.text3, marginBottom: 4, marginTop: 10 }}>负向提示词（可空）</div>
                  <textarea value={pGenForm.negative} onChange={(e) => setPGenForm({ ...pGenForm, negative: e.target.value })} rows={2} placeholder="不希望出现的内容，如 多余手指,低质量（可空）" style={{ width: "100%", boxSizing: "border-box", background: "#12141a", border: `1px solid ${T.line}`, borderRadius: 8, color: T.text1, fontSize: 12, padding: "8px 10px", fontFamily: "Consolas, 'Courier New', monospace", resize: "vertical", lineHeight: 1.6 }} />
                  <div style={{ fontSize: 11, color: T.text3, marginBottom: 4, marginTop: 10 }}>关键参数</div>
                  <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr 1fr", gap: 8 }}>
                    <input value={pGenForm.seed} onChange={(e) => setPGenForm({ ...pGenForm, seed: e.target.value })} placeholder="seed 种子" style={{ padding: "7px 10px", minWidth: 0 }} />
                    <input value={pGenForm.steps} onChange={(e) => setPGenForm({ ...pGenForm, steps: e.target.value })} placeholder="steps 步数" style={{ padding: "7px 10px", minWidth: 0 }} />
                    <input value={pGenForm.cfg} onChange={(e) => setPGenForm({ ...pGenForm, cfg: e.target.value })} placeholder="cfg 引导强度" style={{ padding: "7px 10px", minWidth: 0 }} />
                    <input value={pGenForm.model} onChange={(e) => setPGenForm({ ...pGenForm, model: e.target.value })} placeholder="模型" style={{ padding: "7px 10px", minWidth: 0 }} />
                  </div>
                  <div style={{ display: "flex", gap: 10, marginTop: 16, justifyContent: "flex-end" }}>
                    <button onClick={() => setPGenEditOpen(false)} style={{ height: 32, padding: "0 16px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" }}>取消</button>
                    <button onClick={doGenSave} style={{ height: 32, padding: "0 18px", borderRadius: 8, border: "none", background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505", fontWeight: 650, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" }}>保存</button>
                  </div>
                </div>
              </div>
            );
        const pModalJSX = pModal && (
              <div style={{ position: "fixed", inset: 0, background: "rgba(8,10,14,.66)", zIndex: 120, display: "flex", alignItems: "center", justifyContent: "center" }} onClick={() => setPModal(null)}>
                <div style={{ width: 430, maxWidth: "92vw", border: `1px solid #3a3f4b`, borderRadius: 14, background: T.panel, padding: 20, boxShadow: "0 30px 80px rgba(0,0,0,.6)" }} onClick={(e) => e.stopPropagation()}>
                  <div style={{ fontSize: 15, fontWeight: 650, marginBottom: 4 }}>{pModal === "import" ? "📤 导入立绘素材" : "＋ 新建立绘素材"}</div>
                  <div style={{ fontSize: 11.5, color: T.text3, marginBottom: 14 }}>{pModal === "import" ? `已选择：${pDraft.srcPath.split(/[\/]/).pop()}` : "创建一条素材记录（无图占位，稍后可导入图片）"}</div>
                  <div style={{ display: "grid", gridTemplateColumns: "72px 1fr", gap: "8px 12px", alignItems: "center" }}>
                    <label style={{ fontSize: 12, color: T.text2 }}>名称 *</label>
                    <input value={pDraft.name} onChange={(e) => setPDraft({ ...pDraft, name: e.target.value })} placeholder="例如：雨中对峙" style={{ padding: "7px 10px" }} />
                    {/* 「角色」「版本」已删（2026-10-25）：角色归属统一看「绑定角色」，版本号不再展示 */}
                    {pModal === "create" && (<>
                      <label style={{ fontSize: 12, color: T.text2 }}>比例</label>
                      <input value={pDraft.ratio} onChange={(e) => setPDraft({ ...pDraft, ratio: e.target.value })} placeholder="如 3:4 / 1024×1536（可空）" style={{ padding: "7px 10px" }} />
                    </>)}
                    <label style={{ fontSize: 12, color: T.text2 }}>标签</label>
                    <input value={pDraft.tags} onChange={(e) => setPDraft({ ...pDraft, tags: e.target.value })} placeholder="逗号分隔，如 全身,冷色调" style={{ padding: "7px 10px" }} />
                    <label style={{ fontSize: 12, color: T.text2 }}>分组</label>
                    <input value={pDraft.group} onChange={(e) => setPDraft({ ...pDraft, group: e.target.value })} placeholder="可多个，逗号分隔（新名自动创建）" style={{ padding: "7px 10px" }} />
                  </div>
                  <div style={{ display: "flex", gap: 10, marginTop: 16, justifyContent: "flex-end" }}>
                    <button onClick={() => setPModal(null)} style={{ height: 32, padding: "0 16px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, cursor: "pointer", fontSize: 12.5 }}>取消</button>
                    <button onClick={submitPModal} style={{ height: 32, padding: "0 18px", borderRadius: 8, border: "none", background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505", fontWeight: 650, cursor: "pointer", fontSize: 12.5 }}>{pModal === "import" ? "导入" : "创建"}</button>
                  </div>
                </div>
              </div>
            );

        // ===== 详情页（方案 F） =====
        if (pDetail !== null) {
          const m = portraits.find((x) => x.id === pDetail)!;
          const sameRole = m.character_id ? portraits.filter((x) => x.character_id === m.character_id && x.id !== m.id).slice(0, 4) : [];
          return (
            <>
            <div style={{ maxWidth: 1312, width: "100%", margin: "0 auto", border: `1px solid #3a3f4b`, borderRadius: 10, background: T.win, overflow: "hidden", boxShadow: "0 30px 80px rgba(0,0,0,.55), 0 4px 14px rgba(0,0,0,.4)" }}>
              <div style={{ padding: "14px 24px", background: T.panel, borderBottom: `1px solid ${T.line}`, display: "flex", alignItems: "center", gap: 12 }}>
                <button onClick={() => setPDetail(null)} style={fmtBtn}>← 返回素材库</button>
                <span style={{ fontSize: 13, color: T.text2 }}>素材详情 · {m.name}</span>
                {/* 重出这张：带原提示词与种子（新图挂到重出链上） */}
                <button style={{ ...fmtBtn, marginLeft: "auto" }} onClick={() => regenFromPortrait(m)}
                  title="用这张的提示词与种子再生成一张（新图会记到它的重出链上）">🔁 重出这张</button>
                {/* 删除**这一张**（2026-10-25 用户要求）：素材库原来只能在批量操作条里删 */}
                <button
                  onClick={() => void doDeleteOne(m.id)}
                  title="删除这个素材（记录与图片文件一起删，不可撤销）"
                  style={{ ...fmtBtn, borderColor: "rgba(240,85,77,.4)", color: "#ff8c85" }}>🗑 删除</button>
                {(m.source ?? "").startsWith("ai-") && (
                  <span style={{ fontSize: 10.5, color: "#ffd27d", border: "1px solid rgba(255,210,125,.45)", borderRadius: 6, padding: "2px 7px" }}>
                    AI 生成 · 对外发布须标注
                  </span>
                )}
              </div>
              <div style={{ display: "flex", gap: 16, padding: 16 }}>
                <div style={{ flex: 1, borderRight: `1px solid ${T.line}`, paddingRight: 16, display: "flex", flexDirection: "column", gap: 12, minWidth: 0 }}>
                  {/* 预览框固定 1:1（2026-10-25 用户要求）：宽度撑满左栏，但不超过 60vh，
                      这样正方形在窄窗口里也不会顶到屏幕外面去 */}
                  <div ref={frameRef} style={{ aspectRatio: "1 / 1", width: "100%", maxWidth: "min(100%, 60vh)", margin: "0 auto", borderRadius: 12, position: "relative", overflow: "hidden", display: "flex", alignItems: "center", justifyContent: "center", background: "radial-gradient(circle at 50% 35%,rgba(255,122,24,.12),transparent 62%),linear-gradient(160deg,#4a5f8a,#1e2430)" }}>
                    {/* 徽标层级要在图片**之上**（zIndex: 2）：图片是 absolute inset-0，
                        不设层级时会把左上角那排徽标盖掉（宽图尤其明显，2026-10-25 实测） */}
                    <div style={{ position: "absolute", left: 10, top: 10, display: "flex", gap: 6, zIndex: 2 }}>
                      <span style={{ fontSize: 9.5, padding: "1px 6px", borderRadius: 5, background: T.accentSoft, color: T.accent }}>当前场景使用中</span>
                      {(m.source ?? "").startsWith("ai-") && (
                        <span title="AI 生成内容（对外发布时须标注 AI 生成字样）" style={{ fontSize: 9.5, fontWeight: 700, padding: "1px 6px", borderRadius: 5, background: "rgba(255,210,125,.16)", color: "#ffd27d", border: "1px solid rgba(255,210,125,.45)" }}>AI 生成</span>
                      )}
                      {/* 文件格式按真实扩展名显示（原来写死"透明底 PNG"，对 jpg 是假话）；v1 这种版本号已不再显示 */}
                      {(() => {
                        const ext = (m.path ?? "").split(".").pop() ?? "";
                        return ext && ext.length <= 4
                          ? <span style={{ fontSize: 9.5, padding: "1px 6px", borderRadius: 5, background: "rgba(0,0,0,.55)", color: "#dfe3ec" }}>{ext.toUpperCase()}</span>
                          : null;
                      })()}
                    </div>
                    {m.path ? (
                      <img
                        ref={imgRef}
                        src={convertFileSrc(m.path)}
                        alt={m.name}
                        // 拖拽平移（2026-10-25）：双击还原去掉了，改成"抓手拖动看局部"。
                        // pointer capture 让鼠标拖出图片外面也不断，松手在窗口任意处都成立。
                        onPointerDown={(e) => {
                          if (zoom <= 1) return;                 // 1× 时图已经整张在框里，没有可拖的
                          e.preventDefault();
                          (e.currentTarget as Element).setPointerCapture?.(e.pointerId);
                          dragRef.current = { x: e.clientX, y: e.clientY, ox: offset.x, oy: offset.y };
                          setDragging(true);
                        }}
                        onPointerMove={(e) => {
                          const d = dragRef.current;
                          if (!d) return;
                          setOffset(clampOffset({ x: d.ox + (e.clientX - d.x), y: d.oy + (e.clientY - d.y) }, zoom));
                        }}
                        onPointerUp={(e) => {
                          dragRef.current = null;
                          setDragging(false);
                          (e.currentTarget as Element).releasePointerCapture?.(e.pointerId);
                        }}
                        onPointerCancel={() => { dragRef.current = null; setDragging(false); }}
                        title={zoom > 1 ? "拖动查看其他区域 · 滚轮缩放" : "滚轮放大看细节（检查 AI 出图的手/眼神）"}
                        style={{
                          position: "absolute", inset: 0, width: "100%", height: "100%", objectFit: "contain", display: "block",
                          transform: `translate(${offset.x}px, ${offset.y}px) scale(${zoom})`, transformOrigin: "center center",
                          transition: dragging ? "none" : "transform .12s ease-out",
                          // 鼠标在图上就是"小手"：可拖时 grab，拖动中 grabbing
                          cursor: dragging ? "grabbing" : "grab",
                          // 拖拽时不要触发浏览器原生的图片拖拽/选中
                          userSelect: "none", touchAction: "none",
                        }}
                        draggable={false}
                      />
                    ) : <span style={{ fontSize: 96, color: "rgba(255,255,255,.62)" }}>🧑</span>}
                    {/* 预览操作（2026-10-25）：原来这里是 4 个**点了没反应**的假按钮（🔍- 🔍+ ▦ ⛶）。
                        只保留真有用途的缩放 —— 看 AI 出图的细节；网格/全屏价值低（系统缩放可替代），删掉。 */}
                    {m.path && (
                      <div style={{ position: "absolute", right: 10, bottom: 10, display: "flex", gap: 6, alignItems: "center", zIndex: 2 }}>
                        {zoom > 1 && (
                          <span style={{ fontSize: 9.5, padding: "2px 6px", borderRadius: 5, background: "rgba(0,0,0,.55)", color: "#dfe3ec" }}>
                            {Math.round(zoom * 100)}%
                          </span>
                        )}
                        <span onClick={() => setZoom((z) => clampZoom(z - 0.25))} title="缩小"
                          style={{ width: 28, height: 28, borderRadius: 7, background: "rgba(0,0,0,.55)", display: "flex", alignItems: "center", justifyContent: "center", cursor: zoom <= 1 ? "not-allowed" : "pointer", fontSize: 12, color: "#fff", opacity: zoom <= 1 ? .4 : 1 }}>🔍-</span>
                        <span onClick={() => setZoom((z) => clampZoom(z + 0.25))} title="放大"
                          style={{ width: 28, height: 28, borderRadius: 7, background: "rgba(0,0,0,.55)", display: "flex", alignItems: "center", justifyContent: "center", cursor: zoom >= 4 ? "not-allowed" : "pointer", fontSize: 12, color: "#fff", opacity: zoom >= 4 ? .4 : 1 }}>🔍+</span>
                        <span onClick={() => { setZoom(1); setOffset({ x: 0, y: 0 }); }} title="还原：缩放回 100%、位置回中"
                          style={{ height: 28, padding: "0 8px", borderRadius: 7, background: "rgba(0,0,0,.55)", display: "flex", alignItems: "center", justifyContent: "center", cursor: zoom === 1 && offset.x === 0 && offset.y === 0 ? "not-allowed" : "pointer", fontSize: 10.5, color: "#fff", opacity: zoom === 1 && offset.x === 0 && offset.y === 0 ? .4 : 1 }}>1:1</span>
                      </div>
                    )}
                  </div>
                  <div style={{ display: "flex", gap: 8 }}>
                    {sameRole.map((x) => <div key={x.id} onClick={() => setPDetail(x.id)} style={{ width: 56, height: 70, borderRadius: 8, overflow: "hidden", border: `1px solid ${x.id === m.id ? T.accent : T.line}`, cursor: "pointer", display: "flex", alignItems: "center", justifyContent: "center", fontSize: 20, background: PORTRAIT_GRADS[x.g] }}>{x.path ? <img src={convertFileSrc(x.path)} alt="" style={{ width: "100%", height: "100%", objectFit: "cover", display: "block" }} /> : <span>🧑</span>}</div>)}
                    <div onClick={() => doPick("")} title="导入同角色素材" style={{ width: 56, height: 70, borderRadius: 8, border: `1px dashed ${T.line}`, display: "flex", alignItems: "center", justifyContent: "center", fontSize: 20, color: T.text3, cursor: "pointer" }}>＋</div>
                  </div>
                </div>
                <div style={{ width: 296, flex: "0 0 296px", padding: 16, overflowY: "auto" }}>
                  {/* 标题显示**绑定角色名**（没绑定才退回素材名）；「已定稿」是写死的假徽标，已删（2026-10-25） */}
                  <div style={{ fontSize: 15, fontWeight: 650, display: "flex", alignItems: "center", gap: 7 }}>
                    {m.bound_character || m.name}
                  </div>
                  <div style={{ marginTop: 16 }}>
                    <div style={{ fontSize: 10.5, color: T.text3, letterSpacing: 1, marginBottom: 8, display: "flex", alignItems: "center", gap: 5 }}>ℹ️ 素材信息</div>
                    <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", fontSize: 11.5, padding: "6px 0", borderBottom: `1px dashed ${T.lineSoft}`, flexWrap: "wrap", gap: 6 }}>
                      <span style={{ color: T.text3 }}>绑定角色</span>
                      {m.bound_story && m.bound_character ? <span style={{ color: T.accent, fontWeight: 500 }}>{m.bound_story} · {m.bound_character}</span> : <span style={{ color: T.text3 }}>未绑定</span>}
                      <button onClick={() => { setBindStory(m.story_id || 0); setBindChar(m.character_id || 0); if (m.story_id) loadBindChars(m.story_id); setPBindOpen(true); }} style={{ marginLeft: "auto", height: 24, padding: "0 10px", borderRadius: 6, border: `1px solid ${T.line}`, background: "#191c22", color: m.bound_story ? T.accent : T.text2, cursor: "pointer", fontSize: 11, fontFamily: "inherit" }}>{m.bound_story ? "重新绑定" : "绑定"}</button>
                    </div>
                    {/* 「角色」→「表情标签」（2026-10-25）：角色归属看上面的「绑定角色」，
                        这一行改成表情名（冷静/愠怒…），它就是监视器里表情差分的角标文字 */}
                    <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", fontSize: 11.5, padding: "6px 0", borderBottom: `1px dashed ${T.lineSoft}`, flexWrap: "wrap", gap: 6 }}>
                      <span style={{ color: T.text3 }}>表情标签</span>
                      <span style={{ color: expressionTagOf(m) ? T.accent : T.text3, fontWeight: 500 }}>{expressionTagOf(m) || "未设置"}</span>
                      <button onClick={() => { setPExprVal(expressionTagOf(m)); setPExprOpen(true); }} style={{ marginLeft: "auto", height: 24, padding: "0 10px", borderRadius: 6, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, cursor: "pointer", fontSize: 11, fontFamily: "inherit" }}>{expressionTagOf(m) ? "更换" : "选择"}</button>
                    </div>
                    <div style={{ display: "flex", alignItems: "flex-start", gap: 6, fontSize: 11.5, padding: "6px 0", borderBottom: `1px dashed ${T.lineSoft}`, flexWrap: "wrap" }}>
                      <span style={{ color: T.text3, paddingTop: 2 }}>分组</span>
                      <span style={{ marginLeft: "auto", display: "flex", flexWrap: "wrap", gap: 5, justifyContent: "flex-end", maxWidth: 180 }}>
                        {groupsOf(m).length === 0 && <span style={{ color: T.text1, fontWeight: 500 }}>未分组</span>}
                        {groupsOf(m).map((g) => {
                          const isCharGroup = g === (m.bound_character ?? "");
                          return (
                            <span key={g}
                              title={isCharGroup ? "角色组：由「绑定角色」决定，解绑就消失" : "自定义组：点 ✕ 可移出"}
                              style={{ display: "inline-flex", alignItems: "center", gap: 4, padding: "2px 7px", borderRadius: 10, border: `1px solid ${isCharGroup ? "rgba(255,122,24,.35)" : T.line}`, background: isCharGroup ? T.accentSoft : "#191c22", color: isCharGroup ? T.accent : T.text1, fontSize: 10.5 }}>
                              {g}
                              {!isCharGroup && (
                                <span onClick={() => void removeFromGroup(m.id, g)} style={{ cursor: "pointer", color: T.text3 }}>✕</span>
                              )}
                            </span>
                          );
                        })}
                      </span>
                    </div>
                    {[["分辨率", m.ratio], ["体积", m.size], ["创建时间", m.date], ["来源", (m.source ?? "").startsWith("ai-") ? "AI 生成" : "手工导入"]].map(([k, v]) => (
                      <div key={k} style={{ display: "flex", justifyContent: "space-between", fontSize: 11.5, padding: "5px 0", borderBottom: `1px dashed ${T.lineSoft}` }}><span style={{ color: T.text3 }}>{k}</span><span style={{ color: T.text1, fontWeight: 500 }}>{v || "—"}</span></div>
                    ))}
                  </div>
                  {(() => {
                    // 「表情差分」实装（2026-10-25）：同一角色绑定的立绘按**标签**归组，
                    // 一个标签一个格子（同标签取最新那张），点缩略图切换详情。
                    // 原来是 4 个**假的**占位块（冷静/愠怒/疲惫/苦笑），会让人以为功能已经有了。
                    const sibs = m.character_id ? portraits.filter((x) => x.character_id === m.character_id) : [];
                    const slots = variantSlots(sibs);
                    return (
                      <div style={{ marginTop: 16 }}>
                        <div style={{ fontSize: 10.5, color: T.text3, letterSpacing: 1, marginBottom: 8 }}>
                          😀 表情差分 · {slots.length || 1}
                          <span style={{ marginLeft: 6, letterSpacing: 0 }}>
                            {!m.character_id ? "先绑定角色才能归组" : slots.length > 1 ? "点一张切换" : "这个角色目前只有这一张"}
                          </span>
                        </div>
                        <div style={{ display: "grid", gridTemplateColumns: "repeat(4,1fr)", gap: 7 }}>
                          {(slots.length ? slots.map((s) => s.portrait) : [m]).slice(0, 8).map((x) => (
                            <div key={x.id} onClick={() => setPDetail(x.id)} title={`${x.name}（${x.ver}）`}
                              style={{ height: 52, borderRadius: 8, border: `1px solid ${x.id === m.id ? T.accent : T.line}`, position: "relative", overflow: "hidden", cursor: "pointer", background: x.g ? PORTRAIT_GRADS[x.g] : "linear-gradient(160deg,#4a5f8a,#22293a)" }}>
                              {x.path
                                ? <img src={convertFileSrc(x.path)} alt="" style={{ width: "100%", height: "100%", objectFit: "cover", display: "block" }} />
                                : <span style={{ position: "absolute", inset: 0, display: "flex", alignItems: "center", justifyContent: "center", fontSize: 19 }}>🧑</span>}
                              {labelOfVariant(x) && (
                                <span style={{ position: "absolute", bottom: 0, left: 0, right: 0, textAlign: "center", fontSize: 8.5, color: "#e8eaf0", background: "rgba(0,0,0,.45)", textShadow: "0 1px 2px rgba(0,0,0,.85)" }}>{labelOfVariant(x)}</span>
                              )}
                            </div>
                          ))}
                        </div>
                      </div>
                    );
                  })()}
                  <div style={{ marginTop: 16 }}>
                    <div style={{ fontSize: 10.5, color: T.text3, letterSpacing: 1, marginBottom: 8, display: "flex", alignItems: "center", gap: 5 }}>⌨️ 生成参数
                      <button onClick={() => { setPGenForm({ prompt: m.prompt, negative: m.negative, seed: m.seed, steps: m.steps, cfg: m.cfg, model: m.model }); setPGenEditOpen(true); }} style={{ marginLeft: "auto", height: 24, padding: "0 10px", borderRadius: 6, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, cursor: "pointer", fontSize: 11, fontFamily: "inherit" }}>编辑</button>
                    </div>
                    {m.prompt || m.seed || m.steps || m.cfg || m.model ? (
                      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                        {m.prompt && <div style={{ background: "#12141a", border: `1px solid ${T.line}`, borderRadius: 8, padding: "9px 11px", fontSize: 10.5, lineHeight: 1.75, color: "#9fb0c9", maxHeight: 92, overflowY: "auto", fontFamily: "Consolas, 'Courier New', monospace", whiteSpace: "pre-wrap", wordBreak: "break-word" }}>{m.prompt}</div>}
                        {m.negative && <div style={{ fontSize: 10, color: "#7c8798", lineHeight: 1.6 }}>负向：{m.negative}</div>}
                        <div style={{ display: "flex", gap: 5, flexWrap: "wrap" }}>
                          {[m.seed && `seed ${m.seed}`, m.steps && `steps ${m.steps}`, m.cfg && `cfg ${m.cfg}`, m.model].filter(Boolean).map((t, i) => (
                            <span key={i} style={{ fontSize: 9.5, padding: "2px 7px", borderRadius: 5, background: "rgba(255,122,24,.1)", border: "1px solid rgba(255,122,24,.28)", color: T.accent }}>{t}</span>
                          ))}
                        </div>
                      </div>
                    ) : (
                      <div style={{ fontSize: 11.5, color: T.text3, padding: "10px 0 4px", lineHeight: 1.7 }}>暂无生成参数<br /><span style={{ fontSize: 10, color: "#5b6270" }}>导入素材可手动填写；AI 生图后自动记录。</span></div>
                    )}
                  </div>
                </div>
              </div>
            </div>
            {bindModalJSX}
            {exprJSX}
            {pGenEditJSX}
            {pModalJSX}
            </>
          );
        }

        // ===== 素材库主页面 =====
        return (
          <div style={{ maxWidth: 1312, width: "100%", margin: "0 auto", border: `1px solid #3a3f4b`, borderRadius: 10, background: T.win, overflow: "hidden", boxShadow: "0 30px 80px rgba(0,0,0,.55), 0 4px 14px rgba(0,0,0,.4)" }}>
            <div style={{ padding: "14px 24px 0", background: T.panel, borderBottom: `1px solid ${T.line}` }}>
              <div style={{ display: "flex", alignItems: "flex-start", gap: 16 }}>
                <div>
                  <div style={{ fontSize: 17, fontWeight: 650, display: "flex", alignItems: "center", gap: 9 }}><span style={{ fontSize: 19, color: T.accent }}>🖼</span>立绘素材</div>
                  <div style={{ fontSize: 11.5, color: T.text3, marginTop: 5 }}>
                    共 {portraits.length} 张 · 覆盖 {roles.length - 1} 个角色 · 占用 {fmtTotal} · 最近更新 {recent}
                    {(() => { const ai = portraits.filter((x) => (x.source ?? "").startsWith("ai-")).length;
                      return ai > 0 ? <> · <span style={{ color: T.warn }}>AI 生成 {ai} 张</span></> : null; })()}
                  </div>
                </div>
                <div style={{ marginLeft: "auto", display: "flex", gap: 8, alignItems: "center" }}>
                  <button style={fmtBtn} onClick={() => doPick()}>📤 导入本地</button>
                  <button style={{ ...fmtBtn, background: "linear-gradient(135deg,#FF7A18,#ff9a45)", border: "none", color: "#231505", fontWeight: 650, boxShadow: "0 4px 14px rgba(255,122,24,.24)" }} onClick={() => { setPDraft({ srcPath: "", name: "", role: "", ver: "v1", tags: "", ratio: "", group: "" }); setPModal("create"); }}>＋ 新建素材</button>
                </div>
              </div>
              <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "12px 0 13px", flexWrap: "wrap" }}>
                <div style={{ display: "flex", alignItems: "center", gap: 7, height: 32, padding: "0 11px", width: 230, border: `1px solid ${T.line}`, borderRadius: 8, background: "#191c22", color: T.text3, fontSize: 12 }}>
                  <span style={{ fontSize: 16 }}>🔍</span>
                  <input value={pSearch} onChange={(e) => setPSearch(e.target.value)} placeholder="搜索素材名、角色或标签" style={{ border: "none", background: "transparent", outline: "none", color: T.text1, fontSize: 12, fontFamily: "inherit", width: "100%" }} />
                </div>
                {roles.map((r) => (
                  <button key={r} onClick={() => setPRole(r)} style={chipBtn(pRole === r)}>👥 {r}</button>
                ))}
                {/* 来源筛选：AI 生成的内容按《标识办法》需可识别，这里能一眼筛出来 */}
                {(["全部", "手工", "AI 生成"] as const).map((s) => (
                  <button key={s} onClick={() => setPSource(s)} style={chipBtn(pSource === s)}>
                    {s === "全部" ? "🧾 来源：全部" : s === "手工" ? "🖐 手工" : "🤖 AI 生成"}
                  </button>
                ))}
                {/* 「🏷 标签：全身」「😀 表情差分」两个灰按钮已删（2026-10-25）：
                    它们 disabled + title="规划中"，点了永远没反应，属于假控件。 */}
                <button style={chipBtn(false)} onClick={() => setPSort(pSort === "date" ? "name" : "date")}>↕ 排序：{pSort === "date" ? "最近更新" : "名称"}</button>
                <div style={{ display: "inline-flex", background: "#191c22", border: `1px solid ${T.line}`, borderRadius: 8, padding: 2, marginLeft: "auto" }}>
                  {([["grid", "▦ 网格"], ["masonry", "≡ 瀑布"], ["group", "🗂 分组"]] as const).map(([v, l]) => (
                    <button key={v} onClick={() => setPView(v)} style={{ border: "none", background: "transparent", color: pView === v ? T.accent : T.text3, fontSize: 11.5, padding: "4px 10px", borderRadius: 6, cursor: "pointer", fontFamily: "inherit", display: "flex", alignItems: "center", gap: 4, fontWeight: pView === v ? 600 : 400 }}>{l}</button>
                  ))}
                </div>
              </div>
            </div>

            <div style={{ padding: "22px 24px 40px", minHeight: 420 }}>
              {/* 方案 A：标准网格 */}
              {pView === "grid" && (
                <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(205px, 1fr))", gap: 14 }}>
                  {filtered.map((m) => {
                    const on = selSet.has(m.id);
                    return (
                      <div key={m.id} onClick={() => setPDetail(m.id)} style={{ border: `1px solid ${on ? T.accent : T.line}`, borderRadius: 12, background: "#1e222a", overflow: "hidden", cursor: "pointer", transition: ".16s", boxShadow: on ? "0 0 0 2px rgba(255,122,24,.22)" : "none" }}>
                        <div style={{ height: 170, position: "relative", display: "flex", alignItems: "center", justifyContent: "center", overflow: "hidden", background: PORTRAIT_GRADS[m.g] }}>
                          {m.path ? <img src={convertFileSrc(m.path)} alt={m.name} style={{ position: "absolute", inset: 0, width: "100%", height: "100%", objectFit: "cover", display: "block" }} /> : <span style={{ fontSize: 38, color: "rgba(255,255,255,.72)" }}>🧑</span>}
                          {/* 原来的 v1 徽标已去掉（2026-10-25）：素材只有一个版本时它是纯噪音 */}
                          {(m.source ?? "").startsWith("ai-") && (
                            <span title="AI 生成内容（对外发布时须标注 AI 生成字样）"
                              style={{ position: "absolute", right: 7, bottom: 7, fontSize: 9.5, fontWeight: 700, padding: "1px 6px", borderRadius: 5, background: "rgba(0,0,0,.62)", color: "#ffd27d", border: "1px solid rgba(255,210,125,.45)" }}>
                              AI
                            </span>
                          )}
                          <span style={{ position: "absolute", bottom: 7, left: 7, fontSize: 9.5, padding: "1px 6px", borderRadius: 5, background: "rgba(0,0,0,.5)", color: "#dfe3ec" }}>{expressionTagOf(m)}</span>
                          <span onClick={(e) => { e.stopPropagation(); toggleSel(m.id); }} style={{ position: "absolute", top: 7, left: 7, width: 18, height: 18, borderRadius: 5, border: "1.5px solid rgba(255,255,255,.55)", background: on ? T.accent : "rgba(0,0,0,.35)", display: "flex", alignItems: "center", justifyContent: "center", fontSize: 12, color: on ? "#231505" : "#fff", cursor: "pointer" }}>{on ? "✓" : ""}</span>
                        </div>
                        <div style={{ padding: "9px 10px 10px" }}>
                          <div style={{ fontSize: 12.5, fontWeight: 600, display: "flex", alignItems: "center", gap: 5 }}>{m.bound_character || m.name}{m.pinned && <span style={{ fontSize: 13 }}>📌</span>}</div>
                          <div style={{ fontSize: 11, color: T.text3, marginTop: 3 }}>{m.ratio} · {m.size} · {m.date}</div>
                          <div style={{ display: "flex", flexWrap: "wrap", gap: 4, marginTop: 8 }}>
                            {m.tags.map((t) => <span key={t} style={{ fontSize: 9.5, padding: "2px 6px", borderRadius: 5, ...gtag(t) }}>{t}</span>)}
                          </div>
                        </div>
                      </div>
                    );
                  })}
                  <div style={{ border: `1px dashed ${T.line}`, borderRadius: 12, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 6, color: T.text3, fontSize: 11.5, cursor: "pointer", minHeight: 246, background: "transparent" }} onClick={() => doPick()}>
                    <span style={{ fontSize: 24, color: T.text3 }}>＋</span>上传或生成立绘
                    <span style={{ fontSize: 10, color: "#5b6270" }}>支持 PNG / WEBP，或拖入此区域</span>
                  </div>
                </div>
              )}

              {/* 方案 D：瀑布流 */}
              {pView === "masonry" && (
                <div style={{ display: "grid", gridTemplateColumns: "repeat(3,1fr)", gap: 14, alignItems: "start" }}>
                  {[0, 1, 2].map((col) => (
                    <div key={col} style={{ display: "flex", flexDirection: "column", gap: 14 }}>
                      {filtered.filter((_, i) => i % 3 === col).map((m) => {
                        const h = m.name.includes("全身") ? 250 : m.name.includes("半身") ? 180 : m.name.includes("场景") ? 150 : 120;
                        const rt = m.name.includes("场景") ? "场景 16:9" : m.name.includes("半身") ? "半身 4:5" : m.name.includes("特写") ? "特写 1:1" : "全身 3:4";
                        return (
                          <div key={m.id} onClick={() => setPDetail(m.id)} style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: "#1e222a", overflow: "hidden", cursor: "pointer" }}>
                            <div style={{ height: h, position: "relative", display: "flex", alignItems: "center", justifyContent: "center", background: PORTRAIT_GRADS[m.g] }}>
                              {m.path ? <img src={convertFileSrc(m.path)} alt={m.name} style={{ position: "absolute", inset: 0, width: "100%", height: "100%", objectFit: "cover", display: "block" }} /> : <span style={{ fontSize: 34, color: "rgba(255,255,255,.72)" }}>🧑</span>}
                              <span style={{ position: "absolute", top: 7, left: 7, fontSize: 9.5, padding: "1px 6px", borderRadius: 5, background: "rgba(0,0,0,.55)", color: "#dfe3ec" }}>{rt}</span>
                            </div>
                            <div style={{ padding: "8px 9px", fontSize: 11.5, fontWeight: 600 }}>{m.bound_character || m.name}<span style={{ display: "block", fontSize: 10, color: T.text3, fontWeight: 400, marginTop: 2 }}>
                            {[expressionTagOf(m), m.ratio].filter(Boolean).join(" · ")}
                            {(m.source ?? "").startsWith("ai-") && <span style={{ marginLeft: 5, fontWeight: 700, color: "#ffd27d" }}>AI 生成</span>}
                          </span></div>
                          </div>
                        );
                      })}
                    </div>
                  ))}
                </div>
              )}

              {/* 分组视图：按 group_name 聚合，未分组兜底；组内跨角色（角色筛选先生效） */}
              {pView === "group" && (
                <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
                  {/* 分组 = 角色组（派生）+ 自定义组（可多个）；同一张图会同时出现在它归属的每个组里 */}
                  {groupBuckets(filtered).map(({ name: g, items }) => {
                    const isCharGroup = g !== "未分组" && portraits.some((x) => x.bound_character === g);
                    return (
                      <div key={g} style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: "#1e222a", overflow: "hidden" }}>
                        <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "11px 14px", background: T.panel2, borderBottom: `1px solid ${T.lineSoft}` }}>
                          <div style={{ width: 30, height: 30, borderRadius: 9, display: "flex", alignItems: "center", justifyContent: "center", fontSize: 16, color: "rgba(255,255,255,.85)", background: g === "未分组" ? "#31363f" : T.accentSoft }}>{g === "未分组" ? "🗃" : isCharGroup ? "🎭" : "🗂"}</div>
                          <div>
                            <div style={{ fontSize: 12.5, fontWeight: 650, display: "flex", alignItems: "center", gap: 6 }}>
                              {g}
                              {g === "未分组" && <span style={{ fontSize: 9.5, padding: "1px 6px", borderRadius: 5, background: T.panel3, color: T.text3 }}>兜底</span>}
                              {isCharGroup && <span title="由「绑定角色」派生：绑定到该角色的素材自动进这个组" style={{ fontSize: 9.5, padding: "1px 6px", borderRadius: 5, background: T.accentSoft, color: T.accent }}>角色组</span>}
                            </div>
                            <div style={{ fontSize: 10.5, color: T.text3, marginTop: 2 }}>{items.length} 张素材 · 最近更新 {items[0].date}</div>
                          </div>
                          <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 8 }}>
                            <span style={chipBtn(false)} onClick={() => doPick("", g === "未分组" ? "" : g)}>＋ 添加</span>
                            <span style={{ fontSize: 14, color: T.text3, cursor: "pointer" }}>⌄</span>
                          </div>
                        </div>
                        <div style={{ display: "flex", gap: 10, padding: "12px 14px", overflowX: "auto" }}>
                          {items.map((m) => {
                            const cur = m.tags.includes("当前场景");
                            return (
                              <div key={m.id} onClick={() => setPDetail(m.id)} style={{ width: 74, flex: "0 0 74px", cursor: "pointer" }}>
                                <div style={{ height: 96, borderRadius: 9, overflow: "hidden", border: `1px solid ${cur ? T.accent : T.line}`, boxShadow: cur ? "0 0 0 2px rgba(255,122,24,.2)" : "none", background: PORTRAIT_GRADS[m.g], display: "flex", alignItems: "center", justifyContent: "center", fontSize: 24 }}>{m.path ? <img src={convertFileSrc(m.path)} alt="" style={{ width: "100%", height: "100%", objectFit: "cover", display: "block" }} /> : <span>🧑</span>}</div>
                                <div style={{ textAlign: "center", marginTop: 5 }}>
                                  {expressionTagOf(m) && <span style={{ fontSize: 8.5, padding: "0 5px", borderRadius: 4, background: T.accentSoft, color: T.accent, display: "inline-block", maxWidth: 70, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{expressionTagOf(m) || m.role}</span>}
                                  <p style={{ fontSize: 10, color: T.text2, margin: "3px 0 0", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{m.name.split(" · ")[0]}</p>
                                </div>
                              </div>
                            );
                          })}
                          <div style={{ width: 74, flex: "0 0 74px", border: `1px dashed ${T.line}`, borderRadius: 9, display: "flex", alignItems: "center", justifyContent: "center", fontSize: 20, color: T.text3, cursor: "pointer" }} onClick={() => doPick("", g === "未分组" ? "" : g)}>＋</div>
                        </div>
                      </div>
                    );
                  })}
                </div>
              )}

              {/* 方案 G：空态 */}
              {filtered.length === 0 && (
                <div style={{ border: `1px dashed ${T.line}`, borderRadius: 12, padding: "26px 12px", textAlign: "center", background: "transparent" }}>
                  <div style={{ width: 56, height: 56, borderRadius: 16, background: T.panel3, display: "flex", alignItems: "center", justifyContent: "center", margin: "0 auto 12px", fontSize: 26, color: T.text3 }}>🖼</div>
                  <div style={{ fontSize: 12.5, fontWeight: 600 }}>{portraits.length === 0 ? "还没有立绘素材" : "没有匹配的立绘素材"}</div>
                  <p style={{ fontSize: 11, color: T.text3, marginTop: 5, lineHeight: 1.6 }}>{portraits.length === 0 ? "点右上角「📤 导入本地」导入图片，或「＋ 新建素材」创建占位记录。" : "试试调整搜索词或角色筛选。"}</p>
                </div>
              )}
            </div>

            {/* 方案 B：批量操作条 */}
            {selSet.size > 0 && (
              <div style={{ position: "fixed", left: 216, right: 24, bottom: 14, zIndex: 90, marginTop: 14, border: `1px solid rgba(255,122,24,.35)`, background: "rgba(255,122,24,.08)", borderRadius: 12, padding: "10px 14px", display: "flex", alignItems: "center", gap: 10, backdropFilter: "blur(4px)" }}>
                <span style={{ fontSize: 12, color: T.accent, fontWeight: 650 }}>已选 {selSet.size} 项</span>
                <span style={{ flex: 1 }} />
                <button style={fmtBtn} onClick={() => setPGroupOpen(true)}>🗂 加入分组</button>
                <button style={fmtBtn} onClick={() => setPTagOpen(true)}>🏷 批量打标签</button>
                <button style={fmtBtn} onClick={doPExport}>⬇ 导出</button>
                <button style={{ ...fmtBtn, borderColor: "rgba(240,85,77,.4)", color: "#ff8c85" }} onClick={() => setPDelOpen(true)}>🗑 删除</button>
                <button style={fmtBtn} onClick={() => setPSel(new Set())}>取消选择</button>
              </div>
            )}

            {/* 导入 / 新建素材弹窗 */}
            {pModalJSX}

            {/* 加入分组弹窗（2026-10-25：语义从"移动"改成"加入"—— 一个素材可以同时属于多个组） */}
            {pGroupOpen && (
              <div style={{ position: "fixed", inset: 0, background: "rgba(8,10,14,.66)", zIndex: 122, display: "flex", alignItems: "center", justifyContent: "center" }} onClick={() => setPGroupOpen(false)}>
                <div style={{ width: 430, maxWidth: "92vw", border: `1px solid #3a3f4b`, borderRadius: 14, background: T.panel, padding: 20, boxShadow: "0 30px 80px rgba(0,0,0,.6)" }} onClick={(e) => e.stopPropagation()}>
                  <div style={{ fontSize: 15, fontWeight: 650, marginBottom: 4 }}>🗂 加入分组</div>
                  <div style={{ fontSize: 11.5, color: T.text3, marginBottom: 14 }}>
                    把已选的 {selSet.size} 项「加入」分组 —— 可多属：原来的分组与角色组都保留，只是多挂一个组。
                  </div>
                  <div style={{ display: "flex", flexWrap: "wrap", gap: 7, marginBottom: 12 }}>
                    {Array.from(new Set(portraits.flatMap((m) => customGroupsOf(m)))).sort((a, b) => a.localeCompare(b, "zh")).map((g) => (
                      <button key={g} onClick={() => setPGroupInput(g)} style={{ display: "inline-flex", alignItems: "center", height: 26, padding: "0 12px", borderRadius: 13, border: `1px solid ${pGroupInput === g ? T.accent : T.line}`, background: pGroupInput === g ? T.accentSoft : "#191c22", color: pGroupInput === g ? T.accent : T.text2, cursor: "pointer", fontSize: 11.5, fontFamily: "inherit" }}>{g}</button>
                    ))}
                  </div>
                  <input value={pGroupInput} onChange={(e) => setPGroupInput(e.target.value)} placeholder="输入分组名（新分组将自动创建）" style={{ width: "100%", padding: "7px 10px", boxSizing: "border-box" }} />
                  <div style={{ display: "flex", gap: 10, marginTop: 16, justifyContent: "flex-end" }}>
                    <button onClick={() => setPGroupOpen(false)} style={{ height: 32, padding: "0 16px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" }}>取消</button>
                    <button onClick={doGroupSet} style={{ height: 32, padding: "0 18px", borderRadius: 8, border: "none", background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505", fontWeight: 650, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" }}>确认加入</button>
                  </div>
                </div>
              </div>
            )}

            {/* 批量打标签弹窗 */}
            {pTagOpen && (
              <div style={{ position: "fixed", inset: 0, background: "rgba(8,10,14,.66)", zIndex: 123, display: "flex", alignItems: "center", justifyContent: "center" }} onClick={() => setPTagOpen(false)}>
                <div style={{ width: 430, maxWidth: "92vw", border: `1px solid #3a3f4b`, borderRadius: 14, background: T.panel, padding: 20, boxShadow: "0 30px 80px rgba(0,0,0,.6)" }} onClick={(e) => e.stopPropagation()}>
                  <div style={{ fontSize: 15, fontWeight: 650, marginBottom: 4 }}>🏷 批量打标签</div>
                  <div style={{ fontSize: 11.5, color: T.text3, marginBottom: 14 }}>为已选 {selSet.size} 项素材追加标签（已有标签保留）。</div>
                  <input value={pTagInput} onChange={(e) => setPTagInput(e.target.value)} placeholder="逗号分隔，如 全身,冷色调" style={{ width: "100%", padding: "7px 10px", boxSizing: "border-box" }} />
                  <div style={{ display: "flex", gap: 10, marginTop: 16, justifyContent: "flex-end" }}>
                    <button onClick={() => setPTagOpen(false)} style={{ height: 32, padding: "0 16px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" }}>取消</button>
                    <button onClick={doTagBatch} style={{ height: 32, padding: "0 18px", borderRadius: 8, border: "none", background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505", fontWeight: 650, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" }}>确认添加</button>
                  </div>
                </div>
              </div>
            )}

            {/* 批量删除确认弹窗 */}
            {pDelOpen && (
              <div style={{ position: "fixed", inset: 0, background: "rgba(8,10,14,.66)", zIndex: 124, display: "flex", alignItems: "center", justifyContent: "center" }} onClick={() => setPDelOpen(false)}>
                <div style={{ width: 430, maxWidth: "92vw", border: `1px solid #3a3f4b`, borderRadius: 14, background: T.panel, padding: 20, boxShadow: "0 30px 80px rgba(0,0,0,.6)" }} onClick={(e) => e.stopPropagation()}>
                  <div style={{ fontSize: 15, fontWeight: 650, marginBottom: 4 }}>🗑 删除素材</div>
                  <div style={{ fontSize: 11.5, color: T.text3, marginBottom: 14 }}>将永久删除已选 {selSet.size} 项素材及其图片文件，此操作不可恢复。</div>
                  <div style={{ display: "flex", gap: 10, marginTop: 16, justifyContent: "flex-end" }}>
                    <button onClick={() => setPDelOpen(false)} style={{ height: 32, padding: "0 16px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" }}>取消</button>
                    <button onClick={doDelete} style={{ height: 32, padding: "0 18px", borderRadius: 8, border: "none", background: "linear-gradient(135deg,#e0483e,#f06b5a)", color: "#fff", fontWeight: 650, cursor: "pointer", fontSize: 12.5, fontFamily: "inherit" }}>确认删除</button>
                  </div>
                </div>
              </div>
            )}

            {exprJSX}
            {bindModalJSX}
          </div>
        );
      })()
  );
}