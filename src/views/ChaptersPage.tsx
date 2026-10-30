// 历史章节（2026-10-30，阶段①）：**只读聚合**。
//
// 章是独立实体，作者手动圈定它包含哪几天（多对多）；正文一律回剧情区改，
// 这里只负责“看一眼进度、点开跳过去”。所以本页没有任何写正文的入口。

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import { T } from "../theme";
import { canCreateChapter, ownerChapterOf, selectionSummary, toggleNodeId, wordsLabel } from "../lib/chapters";
import type { Chapter, Story, TimelineNodeDto } from "../types";

type Props = {
  currentStory: Story | null;
  nodes: TimelineNodeDto[];
  setMsg: (s: string) => void;
  /// 跳转：App 负责切到剧情区并加载这个节点
  openChapterNode: (nodeId: number) => void;
};

export function ChaptersPage({ currentStory, nodes, setMsg, openChapterNode }: Props) {
  const [chapters, setChapters] = useState<Chapter[]>([]);
  const [free, setFree] = useState<{ id: number; label: string }[]>([]);
  const [busy, setBusy] = useState(false);
  const [creating, setCreating] = useState(false);
  const [title, setTitle] = useState("");
  const [sel, setSel] = useState<number[]>([]);
  const [confirmDel, setConfirmDel] = useState<number | null>(null);

  const reload = async () => {
    if (!currentStory) { setChapters([]); setFree([]); return; }
    try {
      const list = await invoke<Chapter[]>("list_chapters", { storyId: String(currentStory.id) });
      setChapters(list);
      const days = await invoke<[number, string][]>("list_unassigned_days", { storyId: String(currentStory.id) });
      setFree(days.map(([id, label]) => ({ id, label })));
    } catch (e) {
      setMsg("❌ 章节加载失败：" + String(e));
    }
  };
  useEffect(() => { void reload(); }, [currentStory?.id]);

  const create = async () => {
    if (!currentStory) return;
    const gate = canCreateChapter(title, sel);
    if (!gate.ok) { setMsg("⚠ " + gate.reason); return; }
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
        setMsg("⚠ 章建好了但列表里没找到（id=" + id + "），可能读到了别的分支");
      } else if (made.node_ids.length !== sel.length) {
        setMsg(`⚠ 只收到 ${made.node_ids.length}/${sel.length} 天，检查一下多选传参`);
      } else {
        setMsg(`✅ 已建第 ${made.order_no} 章「${made.title}」（${made.node_ids.length} 天，${wordsLabel(made.words)}）`);
      }
      setCreating(false); setTitle(""); setSel([]);
      await reload();
    } catch (e) {
      setMsg("❌ 建章失败：" + String(e));
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
      setMsg("🗑 已删除这一章（剧情一条没动）");
      await reload();
    } catch (e) {
      setMsg("❌ 删除失败：" + String(e));
    } finally {
      setBusy(false);
    }
  };

  const totalWords = chapters.reduce((s, c) => s + c.words, 0);
  const dayNodes = nodes.filter((n) => (n as { node_type?: string }).node_type !== "interlude");

  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", minWidth: 0, padding: 20, gap: 14, overflowY: "auto" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
        <div style={{ fontSize: 18, fontWeight: 700 }}>📖 历史章节</div>
        <div style={{ fontSize: 12, color: T.text3 }}>
          {chapters.length} 章 · 共 {wordsLabel(totalWords)} · 还有 {free.length} 天没归章
        </div>
        <div style={{ flex: 1 }} />
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
          <div style={{ fontSize: 11.5, color: T.text3 }}>{selectionSummary(sel, dayNodes).text}</div>
          <div style={{ display: "flex", flexWrap: "wrap", gap: 6, maxHeight: 220, overflowY: "auto" }}>
            {dayNodes.length === 0 && <span style={{ color: T.text3, fontSize: 12 }}>这个分支还没有“天”。</span>}
            {dayNodes.map((n) => {
              const on = sel.includes(n.id);
              const owner = ownerChapterOf(n.id, chapters);
              return (
                <button key={n.id} onClick={() => setSel((cur) => toggleNodeId(cur, n.id))}
                  title={owner ? `已被第 ${owner} 章占用（允许重复，但会重复计字数）` : "点一下加入 / 再点移出"}
                  style={{ fontSize: 12, padding: "4px 9px", borderRadius: 8, cursor: "pointer", fontFamily: "inherit",
                    border: `1px solid ${on ? T.accent : T.line}`, background: on ? T.accentSoft : "transparent",
                    color: on ? T.accent : owner ? T.text3 : T.text2 }}>
                  {n.date_label}{owner ? ` · 第${owner}章` : ""}
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

      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        {chapters.map((c) => (
          <div key={c.id} style={{ border: `1px solid ${T.lineSoft}`, borderRadius: 12, padding: 12, display: "flex", gap: 12, alignItems: "center" }}>
            <div style={{ fontSize: 22, fontWeight: 800, color: T.text3, width: 46, textAlign: "center" }}>{String(c.order_no).padStart(2, "0")}</div>
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ fontSize: 14, fontWeight: 600 }}>{c.title || "（未命名）"}</div>
              <div style={{ fontSize: 11.5, color: T.text3, marginTop: 3, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                {c.node_labels.join(" · ") || "（还没关联任何天）"}
              </div>
            </div>
            <div style={{ fontSize: 11.5, color: T.text3, textAlign: "right", flexShrink: 0 }}>
              <div>{wordsLabel(c.words)}</div>
              <div>{c.node_ids.length} 天 · {c.status}</div>
            </div>
            <button onClick={() => { const first = c.node_ids[0]; if (first == null) { setMsg("⚠ 这一章还没关联任何天"); return; } openChapterNode(first); }}
              style={{ height: 28, padding: "0 12px", borderRadius: 8, border: `1px solid ${T.line}`, background: "transparent", color: T.text2, cursor: "pointer", fontFamily: "inherit", flexShrink: 0 }}>
              打开
            </button>
            {confirmDel === c.id ? (
              <>
                <button onClick={() => void remove(c.id)} disabled={busy}
                  style={{ height: 28, padding: "0 10px", borderRadius: 8, border: "none", background: T.danger, color: "#fff", cursor: "pointer", fontFamily: "inherit", flexShrink: 0 }}>确认删</button>
                <button onClick={() => setConfirmDel(null)}
                  style={{ height: 28, padding: "0 10px", borderRadius: 8, border: `1px solid ${T.line}`, background: "transparent", color: T.text2, cursor: "pointer", fontFamily: "inherit", flexShrink: 0 }}>取消</button>
              </>
            ) : (
              <button onClick={() => setConfirmDel(c.id)} title="只删章，剧情一条不动"
                style={{ height: 28, padding: "0 10px", borderRadius: 8, border: `1px solid ${T.line}`, background: "transparent", color: T.text3, cursor: "pointer", fontFamily: "inherit", flexShrink: 0 }}>删除</button>
            )}
          </div>
        ))}
        {currentStory && chapters.length === 0 && (
          <div style={{ color: T.text3, fontSize: 12.5 }}>还没有章。点「＋ 新建章」，把已经写好的几天圈进来。</div>
        )}
      </div>

      {free.length > 0 && (
        <div style={{ borderTop: `1px solid ${T.lineSoft}`, paddingTop: 10, fontSize: 12, color: T.text3 }}>
          还没归章的天：{free.map((f) => f.label).join("、")}
        </div>
      )}
    </div>
  );
}
