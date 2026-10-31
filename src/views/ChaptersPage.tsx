// 历史章节（2026-10-30，阶段①）：**只读聚合**。
//
// 章是独立实体，作者手动圈定它包含哪几天（多对多）；正文一律回剧情区改，
// 这里只负责“看一眼进度、点开跳过去”。所以本页没有任何写正文的入口。

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { T } from "../theme";
import { useToast } from "../toast";
import { canCreateChapter, ownerChapterOf, selectionSummary, toggleNodeId, wordsLabel } from "../lib/chapters";
import type { Chapter, Foreshadow, Story, TimelineNodeDto } from "../types";

type Props = {
  currentStory: Story | null;
  /// 当前分支（null=主线）：**只列这条线上的天** —— 和剧情区/新建分支同一个逻辑，
  /// 否则不同分支的"第 2 天"会挤在一起，看着像重复（用户实测 2026-10-30）
  branchId: string | null;
  nodes: TimelineNodeDto[];
  /// 跳转：App 负责切到剧情区并加载这个节点
  openChapterNode: (nodeId: number) => void;
};

export function ChaptersPage({ currentStory, branchId, nodes, openChapterNode }: Props) {
  const toast = useToast();
  const [chapters, setChapters] = useState<Chapter[]>([]);
  const [free, setFree] = useState<{ id: number; label: string }[]>([]);
  const [busy, setBusy] = useState(false);
  const [creating, setCreating] = useState(false);
  const [title, setTitle] = useState("");
  const [sel, setSel] = useState<number[]>([]);
  const [confirmDel, setConfirmDel] = useState<number | null>(null);
  // 阶段②：大纲编辑（元信息 + 本章目标）+ 注入开关
  const [injectOn, setInjectOn] = useState(true);
  // 伏笔（阶段③）：手标 + AI 提议（可接受/拒绝）+ 逾期现算
  const [fss, setFss] = useState<Foreshadow[]>([]);
  const [fsText, setFsText] = useState("");
  const [fsBusy, setFsBusy] = useState(false);
  // 本地扫描伏笔（2026-10-31）：只在 Ollama 插件开启 + 勾了「伏笔扫描走本地」时显示
  // 只看「扫描伏笔走本地」这一项（辅助项，允许云端主力同时在用）
  const [localScanOn, setLocalScanOn] = useState(false);
  const [localBusy, setLocalBusy] = useState(false);
  const [localProgress, setLocalProgress] = useState("");
  // 排版（2026-10-30）：A 卡片墙 / F 状态看板（C 版本历史暂不做）
  const [layout, setLayout] = useState<"cards" | "board">("cards");
  // 章 ↔ 天 拖拽（2026-10-31）：把某一天的 chip 拖到另一章 = 改归属；拖到「未归章」= 移出
  const [dragOver, setDragOver] = useState<number | "free" | null>(null);
  const [draft, setDraft] = useState<{
    id: number; title: string; act: number; orderNo: number;
    synopsis: string; goal: string; targetWords: number; status: string;
    /// 关联的天（2026-10-31 起可改：set_chapter_nodes 接界面）
    nodeIds: number[];
  } | null>(null);

  const reload = async () => {
    if (!currentStory) { setChapters([]); setFree([]); return; }
    try {
      const list = await invoke<Chapter[]>("list_chapters", { storyId: String(currentStory.id) });
      setChapters(list);
      const days = await invoke<[number, string][]>("list_unassigned_days", { storyId: String(currentStory.id) });
      setFree(days.map(([id, label]) => ({ id, label })));
    } catch (e) {
      toast("❌ 章节加载失败：" + String(e));
    }
  };
  useEffect(() => { void reload(); }, [currentStory?.id]);
  const loadFss = async () => {
    if (!currentStory) { setFss([]); return; }
    try { setFss(await invoke<Foreshadow[]>("list_foreshadows", { storyId: String(currentStory.id) })); }
    catch { setFss([]); }
  };
  useEffect(() => { void loadFss(); }, [currentStory?.id]);

  const addFs = async () => {
    if (!currentStory || !fsText.trim()) return;
    setFsBusy(true);
    try {
      await invoke<number>("create_foreshadow", { storyId: String(currentStory.id), text: fsText.trim(), windowChapters: 3 });
      setFsText("");
      toast("✅ 已记下这条伏笔");
      await loadFss();
    } catch (e) { toast("❌ 伏笔保存失败：" + String(e)); }
    finally { setFsBusy(false); }
  };
  const setFsStatus = async (id: number, status: string) => {
    if (!currentStory) return;
    setFsBusy(true);
    try {
      await invoke<boolean>("update_foreshadow", { storyId: String(currentStory.id), foreshadowId: id, status });
      await loadFss();
    } catch (e) { toast("❌ 伏笔更新失败：" + String(e)); }
    finally { setFsBusy(false); }
  };
  const exportBook = async () => {
    if (!currentStory) return;
    setBusy(true);
    try {
      const path = await invoke<string>("export_chapters", { storyId: String(currentStory.id), format: "md" });
      toast("📚 已导出全书：" + path + (free.length ? `（另有 ${free.length} 天未归章，收在末尾）` : ""));
    } catch (e) { toast("❌ 导出失败：" + String(e)); }
    finally { setBusy(false); }
  };
  const scanFs = async () => {
    if (!currentStory) return;
    setFsBusy(true);
    try {
      const n = await invoke<number>("scan_foreshadow_proposals", { storyId: String(currentStory.id) });
      toast(n > 0 ? `🔍 发现 ${n} 条候选，去下面逐条接受/拒绝` : "🔍 没有发现新候选（宁缺勿滥）");
      await loadFss();
    } catch (e) { toast("❌ 扫描失败：" + String(e)); }
    finally { setFsBusy(false); }
  };
  const delFs = async (id: number) => {
    if (!currentStory) return;
    setFsBusy(true);
    try {
      await invoke<boolean>("delete_foreshadow", { storyId: String(currentStory.id), foreshadowId: id });
      await loadFss();
    } catch (e) { toast("❌ 伏笔删除失败：" + String(e)); }
    finally { setFsBusy(false); }
  };

  useEffect(() => {
    // 注入开关默认开；只有显式 false/0/off 才算关（和后端同口径）
    void invoke<string | null>("get_config", { key: "outline.inject" }).then((v) => {
      setInjectOn(!(v ?? "").trim().toLowerCase().match(/^(false|0|off)$/));
    }).catch(() => setInjectOn(true));
  }, []);

  // 本地扫描伏笔：只在 Ollama 插件开启 + 勾了「伏笔扫描走本地」时显示
  useEffect(() => {
    void invoke<{ foreshadow_scan: boolean }>("ollama_get")
      .then((c) => setLocalScanOn(!!c.foreshadow_scan))
      .catch(() => setLocalScanOn(false));
    const un = listen<{ round: number; from: number; to: number; total: number; added: number }>(
      "foreshadow-local",
      (e) => setLocalProgress(`第 ${e.payload.round} 轮 ｜ 第 ${e.payload.from}~${e.payload.to} 章 ｜ 共 ${e.payload.total} 章 ｜ 已得 ${e.payload.added} 条`),
    );
    return () => { void un.then((f) => f()); };
  }, []);

  const scanLocal = async () => {
    if (!currentStory) return;
    setLocalBusy(true); setLocalProgress("准备中…");
    try {
      const r = await invoke<{ rounds: number; chapters: number; added: number; capped: boolean; note: string }>(
        "scan_foreshadow_local", { storyId: String(currentStory.id) },
      );
      toast(`🧠 本地扫描完成：${r.rounds} 轮 / ${r.chapters} 章，新增 ${r.added} 条${r.capped ? "（达到 20 条上限）" : ""}`);
      setLocalProgress("");
      await loadFss();
    } catch (e) {
      toast("❌ 本地扫描失败：" + String(e));
      setLocalProgress("");
    } finally { setLocalBusy(false); }
  };

  const create = async () => {
    if (!currentStory) return;
    const gate = canCreateChapter(title, sel);
    if (!gate.ok) { toast("⚠ " + gate.reason); return; }
    setBusy(true);
    try {
      const id = await invoke<number>("create_chapter", {
        storyId: String(currentStory.id),
        title: title.trim(),
        nodeIds: sel, // ← 关键：整组 id 数组原样传给后端（顺序按点击顺序）
      });
      // 自检：回读一次，确认后端真的收到了这些天（按钮传参错了这里会立刻暴露）
      const check = await invoke<Chapter[]>("list_chapters", { storyId: String(currentStory.id) });
      const made = check.find((c) => c.id === id);
      if (!made) {
        toast("⚠ 章建好了但列表里没找到（id=" + id + "），可能读到了别的分支");
      } else if (made.node_ids.length !== sel.length) {
        toast(`⚠ 只收到 ${made.node_ids.length}/${sel.length} 天，检查一下多选传参`);
      } else {
        toast(`✅ 已建第 ${made.order_no} 章「${made.title}」（${made.node_ids.length} 天，${wordsLabel(made.words)}）`);
      }
      setCreating(false); setTitle(""); setSel([]);
      await reload();
    } catch (e) {
      toast("❌ 建章失败：" + String(e));
    } finally {
      setBusy(false);
    }
  };

  const remove = async (id: number) => {
    if (!currentStory) return;
    setBusy(true);
    try {
      await invoke<boolean>("delete_chapter", { storyId: String(currentStory.id), chapterId: id });
      setConfirmDel(null);
      toast("🗑 已删除这一章（剧情一条没动）");
      await reload();
    } catch (e) {
      toast("❌ 删除失败：" + String(e));
    } finally {
      setBusy(false);
    }
  };


  const toggleInject = async (next: boolean) => {
    setInjectOn(next);
    try {
      await invoke("set_config", { key: "outline.inject", value: next ? "true" : "false" });
      toast(next ? "🧭 本章目标：已开启注入" : "🧭 本章目标：注入已关闭（目标仍保存，只是不发）");
    } catch (e) {
      toast("❌ 开关保存失败：" + String(e));
    }
  };

  const advance = async (c: Chapter) => {
    if (!currentStory) return;
    const next = c.status === "draft" ? "revising" : c.status === "revising" ? "final" : "draft";
    setBusy(true);
    try {
      await invoke<boolean>("update_chapter", { storyId: String(currentStory.id), chapterId: c.id, status: next });
      toast(next === "final" ? `✅ 第 ${c.order_no} 章已定稿` : `🧭 第 ${c.order_no} 章 → ${next === "revising" ? "修订中" : "草稿"}`);
      await reload();
    } catch (e) { toast("❌ 状态更新失败：" + String(e)); }
    finally { setBusy(false); }
  };
  const startEdit = (c: Chapter) => {
    setDraft({
      id: c.id, title: c.title, act: c.act, orderNo: c.order_no,
      synopsis: c.synopsis, goal: c.goal, targetWords: c.target_words, status: c.status,
      nodeIds: [...c.node_ids],
    });
  };
  const saveDraft = async () => {
    if (!currentStory || !draft) return;
    setBusy(true);
    try {
      await invoke<boolean>("update_chapter", {
        storyId: String(currentStory.id),
        chapterId: draft.id,
        title: draft.title,
        status: draft.status,
        synopsis: draft.synopsis,
        goal: draft.goal,
        targetWords: draft.targetWords,
        act: draft.act,
        orderNo: draft.orderNo,
      });
      // 关联的天：整组替换（后端独占，会把被抢的天从别的章摘掉）
      await invoke("set_chapter_nodes", {
        storyId: String(currentStory.id),
        chapterId: draft.id,
        nodeIds: draft.nodeIds,
      });
      toast("✅ 章已保存" + (draft.goal.trim() ? "（本章目标会在生成时注入）" : "") + `（${draft.nodeIds.length} 天）`);
      setDraft(null);
      await reload();
    } catch (e) {
      toast("❌ 保存失败：" + String(e));
    } finally {
      setBusy(false);
    }
  };
  /// 拖拽落点：toChapter=null 表示拖到「未归章」
  const moveDay = async (nodeId: number, fromChapter: number | null, toChapter: number | null) => {
    if (!currentStory || fromChapter === toChapter) return;
    setBusy(true);
    try {
      if (toChapter != null) {
        const target = chapters.find((c) => c.id === toChapter);
        if (!target) return;
        await invoke("set_chapter_nodes", {
          storyId: String(currentStory.id),
          chapterId: toChapter,
          nodeIds: [...target.node_ids, nodeId],
        });
        toast(`🔀 已移到第 ${target.order_no} 章`);
      } else if (fromChapter != null) {
        const src = chapters.find((c) => c.id === fromChapter);
        if (!src) return;
        await invoke("set_chapter_nodes", {
          storyId: String(currentStory.id),
          chapterId: fromChapter,
          nodeIds: src.node_ids.filter((id) => id !== nodeId),
        });
        toast("↩ 已移回「未归章」");
      }
      await reload();
    } catch (e) {
      toast("❌ 移动失败：" + String(e));
    } finally {
      setBusy(false);
      setDragOver(null);
    }
  };
  const totalWords = chapters.reduce((s, c) => s + c.words, 0);
  // 一天只属于一章（2026-10-30 用户定）：已被别的章圈走的天，新建时**直接不列出来**
  const dayNodes = nodes
    .filter((n) => (n as { node_type?: string }).node_type !== "interlude")
    // 只留当前分支的节点（node.branch_id 为 null/undefined = 主线）
    .filter((n) => ((n.branch_id ?? null) === (branchId ?? null)))
    .filter((n) => ownerChapterOf(n.id, chapters) === null);

  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", minWidth: 0, padding: 20, gap: 14, overflowY: "auto" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
        <div style={{ fontSize: 18, fontWeight: 700 }}>📖 历史章节</div>
        <div style={{ fontSize: 12, color: T.text3 }}>
          {chapters.length} 章 · 共 {wordsLabel(totalWords)} · 还有 {free.length} 天没归章
        </div>
        <label style={{ fontSize: 11.5, color: T.text2, display: "flex", alignItems: "center", gap: 6, cursor: "pointer" }}
          title="开：生成时把当前章的「本章目标」发进去（目标为空则不发）；关：只保存，不注入">
          <input type="checkbox" checked={injectOn} onChange={(e) => void toggleInject(e.target.checked)} />
          本章目标注入
        </label>
        <div style={{ flex: 1 }} />
        {(["cards", "board"] as const).map((k) => (
          <button key={k} onClick={() => setLayout(k)}
            style={{ height: 28, padding: "0 10px", borderRadius: 8, cursor: "pointer", fontFamily: "inherit", fontSize: 11.5,
              border: `1px solid ${layout === k ? T.accent : T.line}`, background: layout === k ? T.accentSoft : "transparent",
              color: layout === k ? T.accent : T.text2 }}>
            {k === "cards" ? "卡片墙" : "状态看板"}
          </button>
        ))}
        <button onClick={() => void exportBook()} disabled={!currentStory || busy || chapters.length === 0}
          title="按章序拼成一本（md）：章标题作标题、天不出现；未归章的天收在末尾"
          style={{ height: 30, padding: "0 12px", borderRadius: 8, border: `1px solid ${T.line}`, background: "transparent", color: T.text2, cursor: "pointer", fontFamily: "inherit", opacity: !currentStory || busy || chapters.length === 0 ? 0.5 : 1 }}>
          📚 导出全书
        </button>
        <button onClick={() => { setCreating((v) => !v); setSel([]); setTitle(""); }} disabled={!currentStory || busy}
          style={{ height: 30, padding: "0 14px", borderRadius: 8, border: "none", background: T.accent, color: "#0b0d12", fontWeight: 700, cursor: "pointer", fontFamily: "inherit" }}>
          ＋ 新建章
        </button>
      </div>

      {!currentStory && <div style={{ color: T.text3 }}>先在「故事列表」里选一个故事。</div>}

      {creating && (
        <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, padding: 14, display: "flex", flexDirection: "column", gap: 10 }}>
          <input
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            placeholder="这一章叫什么（如：序雾）"
            style={{ height: 32, padding: "0 10px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontFamily: "inherit" }}
          />
          <div style={{ fontSize: 11.5, color: T.text3 }}>
            {selectionSummary(sel, dayNodes).text}
            <span style={{ marginLeft: 8 }}>
              （只列{branchId ? "当前分支" : "主线"}上的天；已被别的章占用的天不再列出：一天只属于一章）
            </span>
          </div>
          <div style={{ display: "flex", flexWrap: "wrap", gap: 6, maxHeight: 220, overflowY: "auto" }}>
            {dayNodes.length === 0 && <span style={{ color: T.text3, fontSize: 12 }}>这个分支还没有“天”。</span>}
            {dayNodes.map((n) => {
              const on = sel.includes(n.id);
              return (
                <button key={n.id} onClick={() => setSel((cur) => toggleNodeId(cur, n.id))}
                  title="点一下加入 / 再点移出"
                  style={{ fontSize: 12, padding: "4px 9px", borderRadius: 8, cursor: "pointer", fontFamily: "inherit",
                    border: `1px solid ${on ? T.accent : T.line}`, background: on ? T.accentSoft : "transparent",
                    color: on ? T.accent : T.text2 }}>
                  {n.date_label}
                </button>
              );
            })}
          </div>
          <div style={{ display: "flex", gap: 8 }}>
            <button onClick={() => void create()} disabled={busy || !canCreateChapter(title, sel).ok}
              style={{ height: 30, padding: "0 14px", borderRadius: 8, border: "none", background: T.accent, color: "#0b0d12", fontWeight: 700, cursor: "pointer", fontFamily: "inherit", opacity: busy || !canCreateChapter(title, sel).ok ? 0.5 : 1 }}>
              创建
            </button>
            <button onClick={() => { setCreating(false); setSel([]); setTitle(""); }}
              style={{ height: 30, padding: "0 14px", borderRadius: 8, border: `1px solid ${T.line}`, background: "transparent", color: T.text2, cursor: "pointer", fontFamily: "inherit" }}>
              取消
            </button>
          </div>
        </div>
      )}

      {draft && (
        <div style={{ border: `1px solid ${T.accent}`, borderRadius: 12, padding: 14, display: "flex", flexDirection: "column", gap: 8 }}>
          <div style={{ fontSize: 12, color: T.accent, fontWeight: 700 }}>编辑第 {draft.orderNo} 章 · 第 {draft.act} 幕</div>
          <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
            <input value={draft.title} onChange={(e) => setDraft({ ...draft, title: e.target.value })} placeholder="标题"
              style={{ flex: 2, minWidth: 160, height: 30, padding: "0 9px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontFamily: "inherit" }} />
            <input type="number" value={draft.act} onChange={(e) => setDraft({ ...draft, act: Number(e.target.value) || 1 })} title="幕"
              style={{ width: 70, height: 30, padding: "0 9px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontFamily: "inherit" }} />
            <input type="number" value={draft.orderNo} onChange={(e) => setDraft({ ...draft, orderNo: Number(e.target.value) || 1 })} title="章序"
              style={{ width: 70, height: 30, padding: "0 9px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontFamily: "inherit" }} />
            <input type="number" value={draft.targetWords} onChange={(e) => setDraft({ ...draft, targetWords: Number(e.target.value) || 0 })} title="目标字数"
              style={{ width: 100, height: 30, padding: "0 9px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontFamily: "inherit" }} />
            <select value={draft.status} onChange={(e) => setDraft({ ...draft, status: e.target.value })}
              style={{ height: 30, padding: "0 8px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontFamily: "inherit" }}>
              <option value="draft">草稿</option>
              <option value="revising">修订中</option>
              <option value="final">已定稿</option>
            </select>
          </div>
          <input value={draft.synopsis} onChange={(e) => setDraft({ ...draft, synopsis: e.target.value })} placeholder="一句话梗概"
            style={{ height: 30, padding: "0 9px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontFamily: "inherit" }} />
          <textarea value={draft.goal} onChange={(e) => setDraft({ ...draft, goal: e.target.value })}
            placeholder="本章目标（会注入生成；留空则不发）——比如：把钥匙交出去，但不许她先开口"
            rows={3}
            style={{ padding: "8px 9px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontFamily: "inherit", resize: "vertical" }} />
          {/* 关联的天（2026-10-31）：可增删；已被别的章占用的天不列出来（一天只属于一章） */}
          <div style={{ fontSize: 11, color: T.text3 }}>关联的天（{draft.nodeIds.length}）· 点一下加入/移出</div>
          <div style={{ display: "flex", flexWrap: "wrap", gap: 6, maxHeight: 180, overflowY: "auto" }}>
            {(() => {
              const own = nodes.filter((n) => draft.nodeIds.includes(n.id));
              const free = dayNodes.filter((n) => !draft.nodeIds.includes(n.id));
              const all = [...own, ...free].sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0));
              if (all.length === 0) return <span style={{ color: T.text3, fontSize: 12 }}>这条分支还没有“天”。</span>;
              return all.map((n) => {
                const on = draft.nodeIds.includes(n.id);
                return (
                  <button key={n.id} onClick={() => setDraft({ ...draft, nodeIds: toggleNodeId(draft.nodeIds, n.id) })}
                    style={{ fontSize: 12, padding: "4px 9px", borderRadius: 8, cursor: "pointer", fontFamily: "inherit",
                      border: `1px solid ${on ? T.accent : T.line}`, background: on ? T.accentSoft : "transparent",
                      color: on ? T.accent : T.text2 }}>
                    {n.date_label}
                  </button>
                );
              });
            })()}
          </div>
          <div style={{ display: "flex", gap: 8 }}>
            <button onClick={() => void saveDraft()} disabled={busy}
              style={{ height: 30, padding: "0 14px", borderRadius: 8, border: "none", background: T.accent, color: "#0b0d12", fontWeight: 700, cursor: "pointer", fontFamily: "inherit" }}>保存</button>
            <button onClick={() => setDraft(null)}
              style={{ height: 30, padding: "0 14px", borderRadius: 8, border: `1px solid ${T.line}`, background: "transparent", color: T.text2, cursor: "pointer", fontFamily: "inherit" }}>取消</button>
          </div>
        </div>
      )}

      {/* 统计卡（2026-10-30，按原型图一）：完成进度 / 累计字数 / 未归章 / 伏笔。
          注意：原型的「历史版本」「AI 生成占比」我们**没有数据源**（无章快照、消息无来源标记），
          先用这两张替代；要真做需要先加表，见 ROADMAP。 */}
      {currentStory && (
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(180px, 1fr))", gap: 10 }}>
          {(() => {
            const done = chapters.filter((c) => c.status === "final").length;
            const total = chapters.length;
            const pct = total ? Math.round((done / total) * 100) : 0;
            const targetSum = chapters.reduce((s, c) => s + c.target_words, 0);
            const overdue = fss.filter((f) => f.overdue).length;
            const card = (title: string, big: string, sub: string, color: string, ratio: number) => (
              <div key={title} style={{ border: `1px solid ${T.lineSoft}`, borderRadius: 12, padding: "10px 12px" }}>
                <div style={{ fontSize: 10.5, color: T.text3, letterSpacing: 0.4 }}>{title}</div>
                <div style={{ fontSize: 20, fontWeight: 800, marginTop: 4 }}>{big}</div>
                <div style={{ height: 4, borderRadius: 2, background: "rgba(255,255,255,.07)", margin: "8px 0 5px" }}>
                  <div style={{ width: `${Math.max(0, Math.min(100, ratio))}%`, height: "100%", background: color, borderRadius: 2 }} />
                </div>
                <div style={{ fontSize: 10.5, color: T.text3 }}>{sub}</div>
              </div>
            );
            return [
              card("完成进度", `${done} / ${total} 章`, `已定稿 ${pct}%`, T.accent, pct),
              card("累计字数", wordsLabel(totalWords), targetSum ? `目标 ${wordsLabel(targetSum)}` : "还没设目标字数", "#5b8cff", targetSum ? Math.min(100, (totalWords / targetSum) * 100) : 0),
              card("未归章", `${free.length} 天`, free.length ? "记得圈进章里" : "全部已归章", T.ok, chapters.length + free.length ? (chapters.length / (chapters.length + free.length)) * 100 : 100),
              card("伏笔", `${fss.length} 条`, overdue ? `逾期 ${overdue} 条` : "无逾期", overdue ? T.danger : T.text3, fss.length ? ((fss.length - overdue) / fss.length) * 100 : 100),
            ];
          })()}
        </div>
      )}

      {chapters.length === 0 && currentStory && (
        <div style={{ color: T.text3, fontSize: 12.5 }}>还没有章。点「＋ 新建章」，把已经写好的几天圈进来。</div>
      )}

      {/* A 卡片墙 */}
      {layout === "cards" && chapters.length > 0 && (
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(260px, 1fr))", gap: 10 }}>
          {chapters.map((c) => (
            <div key={c.id}
              onDragEnter={(e) => { e.preventDefault(); e.stopPropagation(); setDragOver(c.id); }}
              onDragOver={(e) => { e.preventDefault(); e.stopPropagation(); e.dataTransfer.dropEffect = "move"; setDragOver(c.id); }}
              onDragLeave={(e) => { e.stopPropagation(); setDragOver((v) => (v === c.id ? null : v)); }}
              onDrop={(e) => {
                e.preventDefault();
                e.stopPropagation();
                console.log("[chapter-drop]", e.dataTransfer.getData("text/plain"));
                try {
                  const d = JSON.parse(e.dataTransfer.getData("text/plain") || "{}");
                  if (typeof d.nodeId === "number") void moveDay(d.nodeId, d.from ?? null, c.id);
                } catch { /* 非本页拖动，忽略 */ }
              }}
              style={{ border: `1px solid ${dragOver === c.id ? T.accent : T.lineSoft}`, borderRadius: 12, padding: 12, display: "flex", flexDirection: "column", gap: 8,
                background: dragOver === c.id ? T.accentSoft : "transparent" }}>
              <div style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
                <span style={{ fontSize: 26, fontWeight: 900, color: T.text3, opacity: 0.5 }}>{String(c.order_no).padStart(2, "0")}</span>
                <span style={{ fontSize: 14, fontWeight: 700, flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{c.title || "（未命名）"}</span>
                <span style={{ fontSize: 10, color: T.text3 }}>第 {c.act} 幕</span>
              </div>
              <div style={{ fontSize: 11.5, color: T.text3, minHeight: 32, display: "-webkit-box", WebkitLineClamp: 2, WebkitBoxOrient: "vertical", overflow: "hidden" } as React.CSSProperties}>
                {c.synopsis || c.node_labels.join(" · ") || "（还没梗概）"}
              </div>
              <div style={{ display: "flex", gap: 10, fontSize: 10.5, color: T.text3 }}>
                <span>{wordsLabel(c.words)}</span><span>{c.node_ids.length} 天</span>
                <span style={{ color: c.status === "final" ? T.ok : c.status === "revising" ? T.warn : T.text3 }}>
                  {c.status === "final" ? "已定稿" : c.status === "revising" ? "修订中" : "草稿"}
                </span>
                {c.goal.trim() && <span style={{ color: T.accent }}>有目标</span>}
              </div>
              {/* 这一天属于哪一章：直接拖走 / 拖进来（2026-10-31） */}
              <div style={{ display: "flex", flexWrap: "wrap", gap: 5 }}>
                {c.node_ids.map((nid, i) => (
                  <span key={nid} draggable
                    onDragStart={(e) => { e.dataTransfer.setData("text/plain", JSON.stringify({ nodeId: nid, from: c.id })); e.dataTransfer.effectAllowed = "move"; }}
                    title="拖到另一章 = 改归属；拖到下方「未归章」= 移出"
                    style={{ fontSize: 11, padding: "2px 7px", borderRadius: 6, border: `1px solid ${T.line}`, color: T.text3, cursor: "grab" }}>
                    {c.node_labels[i] ?? `#${nid}`}
                  </span>
                ))}
                {c.node_ids.length === 0 && <span style={{ fontSize: 11, color: T.text3 }}>（还没关联天，把下面的天拖进来）</span>}
              </div>
              <div style={{ display: "flex", gap: 6 }}>
                <button onClick={() => { const f = c.node_ids[0]; if (f == null) { toast("⚠ 这一章还没关联任何天"); return; } openChapterNode(f); }}
                  style={{ flex: 1, height: 26, borderRadius: 7, border: "none", background: T.accentSoft, color: T.accent, cursor: "pointer", fontFamily: "inherit", fontSize: 11 }}>打开</button>
                <button onClick={() => startEdit(c)}
                  style={{ height: 26, padding: "0 10px", borderRadius: 7, border: `1px solid ${T.line}`, background: "transparent", color: T.text2, cursor: "pointer", fontFamily: "inherit", fontSize: 11 }}>编辑</button>
                {confirmDel === c.id ? (<>
                  <button onClick={() => void remove(c.id)} disabled={busy}
                    style={{ height: 26, padding: "0 8px", borderRadius: 7, border: "none", background: T.danger, color: "#fff", cursor: "pointer", fontFamily: "inherit", fontSize: 11 }}>确认删</button>
                  <button onClick={() => setConfirmDel(null)}
                    style={{ height: 26, padding: "0 8px", borderRadius: 7, border: `1px solid ${T.line}`, background: "transparent", color: T.text3, cursor: "pointer", fontFamily: "inherit", fontSize: 11 }}>取消</button>
                </>) : (
                  <button onClick={() => setConfirmDel(c.id)} title="只删章，剧情不动"
                    style={{ height: 26, padding: "0 8px", borderRadius: 7, border: `1px solid ${T.line}`, background: "transparent", color: T.text3, cursor: "pointer", fontFamily: "inherit", fontSize: 11 }}>删</button>
                )}
              </div>
            </div>
          ))}
        </div>
      )}

      {/* F 状态看板：三列，点卡片推进状态（草稿 → 修订中 → 已定稿） */}
      {layout === "board" && chapters.length > 0 && (
        <div style={{ display: "grid", gridTemplateColumns: "repeat(3, minmax(0, 1fr))", gap: 10 }}>
          {([["draft", "草稿"], ["revising", "修订中"], ["final", "已定稿"]] as const).map(([st, label]) => (
            <div key={st} style={{ border: `1px solid ${T.lineSoft}`, borderRadius: 12, padding: 10, display: "flex", flexDirection: "column", gap: 8, minHeight: 120 }}>
              <div style={{ fontSize: 11.5, color: T.text2, fontWeight: 700 }}>
                {label} <span style={{ color: T.text3, fontWeight: 400 }}>{chapters.filter((c) => c.status === st).length}</span>
              </div>
              {chapters.filter((c) => c.status === st).map((c) => (
                <button key={c.id} onClick={() => void advance(c)} title="点一下推进到下一状态"
                  style={{ textAlign: "left", border: `1px solid ${T.lineSoft}`, borderRadius: 10, padding: "8px 9px", background: "transparent", color: T.text2, cursor: "pointer", fontFamily: "inherit" }}>
                  <div style={{ fontSize: 12, fontWeight: 600, color: T.text1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{String(c.order_no).padStart(2, "0")} {c.title || "（未命名）"}</div>
                  <div style={{ fontSize: 10.5, color: T.text3, marginTop: 3 }}>{wordsLabel(c.words)} · {c.node_ids.length} 天</div>
                </button>
              ))}
            </div>
          ))}
        </div>
      )}

      {currentStory && (
        <div style={{ borderTop: `1px solid ${T.lineSoft}`, paddingTop: 12, display: "flex", flexDirection: "column", gap: 8 }}>
          <div style={{ fontSize: 13, fontWeight: 700, display: "flex", alignItems: "center", gap: 8 }}>
            伏笔 <span style={{ fontSize: 11, color: T.text3, fontWeight: 400 }}>
              {fss.length} 条 · 逾期 {fss.filter((f) => f.overdue).length} 条
            </span>
            <div style={{ flex: 1 }} />
            <button onClick={() => void scanFs()} disabled={fsBusy} title="扫已归章的天摘要里「答应/约定/没说/没人解释」这类强信号；结果进「提议」"
              style={{ height: 24, padding: "0 10px", borderRadius: 6, border: `1px solid ${T.line}`, background: "transparent", color: T.text2, cursor: "pointer", fontFamily: "inherit", fontSize: 10.5 }}>
              扫描候选
            </button>
            {localScanOn && (
              <button onClick={() => void scanLocal()} disabled={fsBusy || localBusy}
                title="用本地 Ollama 按章扫（每批 2 章：全部日摘要 + 本批原文）；结果进同一个「提议」列表"
                style={{ height: 24, padding: "0 10px", borderRadius: 6, border: "none", background: T.accentSoft, color: T.accent, cursor: localBusy ? "wait" : "pointer", fontFamily: "inherit", fontSize: 10.5 }}>
                {localBusy ? "本地扫描中…" : "🧠 本地扫描伏笔"}
              </button>
            )}
          </div>
          {localBusy && <div style={{ fontSize: 11, color: T.text3 }}>{localProgress}</div>}
          <div style={{ display: "flex", gap: 8 }}>
            <input value={fsText} onChange={(e) => setFsText(e.target.value)}
              placeholder="埋了什么？（当时不知道的东西：一把钥匙的齿形 / 她没说完的半句…）"
              onKeyDown={(e) => { if (e.key === "Enter") void addFs(); }}
              style={{ flex: 1, height: 30, padding: "0 9px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontFamily: "inherit" }} />
            <button onClick={() => void addFs()} disabled={fsBusy || !fsText.trim()}
              style={{ height: 30, padding: "0 14px", borderRadius: 8, border: "none", background: T.accent, color: "#0b0d12", fontWeight: 700, cursor: "pointer", fontFamily: "inherit", opacity: fsBusy || !fsText.trim() ? 0.5 : 1 }}>
              记下
            </button>
          </div>
          {fss.map((f) => {
            const chip: Record<string, string> = { proposed: "提议", planted: "埋下", payoff: "已回收", dropped: "废弃" };
            return (
              <div key={f.id} style={{ display: "flex", gap: 8, alignItems: "center", fontSize: 12 }}>
                <span style={{ flexShrink: 0, width: 52, fontSize: 10.5, color: f.overdue ? T.danger : f.status === "proposed" ? T.accent : T.text3 }}>
                  {f.overdue ? "逾期" : chip[f.status] ?? f.status}
                </span>
                <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", color: f.status === "dropped" ? T.text3 : T.text2 }} title={f.text}>{f.text}</span>
                <span style={{ flexShrink: 0, fontSize: 10, color: T.text3 }}>{f.planted_order ? `第${f.planted_order}章` : "—"}</span>
                {f.status === "proposed" && (<>
                  <button onClick={() => void setFsStatus(f.id, "planted")} disabled={fsBusy} style={{ height: 24, padding: "0 8px", borderRadius: 6, border: "none", background: T.ok, color: "#0b0d12", cursor: "pointer", fontFamily: "inherit", fontSize: 10.5 }}>接受</button>
                  <button onClick={() => void setFsStatus(f.id, "dropped")} disabled={fsBusy} style={{ height: 24, padding: "0 8px", borderRadius: 6, border: `1px solid ${T.line}`, background: "transparent", color: T.text3, cursor: "pointer", fontFamily: "inherit", fontSize: 10.5 }}>拒绝</button>
                </>)}
                {f.status === "planted" && (
                  <button onClick={() => void setFsStatus(f.id, "payoff")} disabled={fsBusy} style={{ height: 24, padding: "0 8px", borderRadius: 6, border: `1px solid ${T.line}`, background: "transparent", color: T.text2, cursor: "pointer", fontFamily: "inherit", fontSize: 10.5 }}>已回收</button>
                )}
                <button onClick={() => void delFs(f.id)} disabled={fsBusy} title="彻底删除（拒绝提议请用「拒绝」，免得重复提）"
                  style={{ height: 24, padding: "0 6px", borderRadius: 6, border: "none", background: "transparent", color: T.text3, cursor: "pointer", fontFamily: "inherit", fontSize: 10.5 }}>×</button>
              </div>
            );
          })}
          {fss.length === 0 && <div style={{ fontSize: 11.5, color: T.text3 }}>还没有伏笔。埋下的东西越具体，后面越好回收。</div>}
        </div>
      )}

      {/* 「未归章」：既是拖拽来源（拖进任意章），也是落点（把章里的天拖回来 = 移出） */}
      {currentStory && (
        <div
          onDragEnter={(e) => { e.preventDefault(); e.stopPropagation(); setDragOver("free"); }}
          onDragOver={(e) => { e.preventDefault(); e.stopPropagation(); e.dataTransfer.dropEffect = "move"; setDragOver("free"); }}
          onDragLeave={() => setDragOver((v) => (v === "free" ? null : v))}
          onDrop={(e) => {
            e.preventDefault();
            try {
              const d = JSON.parse(e.dataTransfer.getData("text/plain") || "{}");
              if (typeof d.nodeId === "number") void moveDay(d.nodeId, d.from ?? null, null);
            } catch { /* ignore */ }
          }}
          style={{ borderTop: `1px solid ${T.lineSoft}`, paddingTop: 10, fontSize: 12, color: T.text3,
            display: "flex", flexWrap: "wrap", gap: 6, alignItems: "center",
            background: dragOver === "free" ? "rgba(255,122,24,.06)" : "transparent" }}>
          <span>还没归章的天（拖进章里，或把章里的天拖回这里）：</span>
          {free.map((f) => (
            <span key={f.id} draggable
              onDragStart={(e) => { e.dataTransfer.setData("text/plain", JSON.stringify({ nodeId: f.id, from: null })); e.dataTransfer.effectAllowed = "move"; }}
              title="拖到某一章 = 归进那一章"
              style={{ fontSize: 11.5, padding: "2px 8px", borderRadius: 6, border: `1px solid ${T.line}`, color: T.text2, cursor: "grab" }}>
              {f.label}
            </span>
          ))}
          {free.length === 0 && <span>（全部已归章）</span>}
        </div>
      )}
    </div>
  );
}
