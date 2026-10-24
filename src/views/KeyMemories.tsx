// 「🧠 关键记忆」面板（v3.4）。
//
// ## 它是什么
//
// 日摘要记"那天大概发生了什么"；关键记忆记**不可重来的事**：表白 / 承诺 / 牺牲 / 背叛。
// 由系统从剧情里提炼（一个事件一个 Markdown 文档），**没有"新建"入口** —— 用户只能看、改、删。
//
// ## 为什么单独一个组件
//
// 它同时要处理三件事：读列表（可能为空）、改正文（写成文件）、导出（生成 `key-memories/` 文件夹）。
// 塞进时间线页会让那一页再涨 150 行；而这块以后还可能长（比如按类型筛选、按天分组），
// 单独一个文件更省。

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { memoryBodyView } from "../lib/memory";
import { T } from "../theme";

type MemoryDto = {
  id: string;
  kind: string;
  label: string;
  day_label: string;
  branch: string | null;
  importance: number;
  body: string;
  created_at: string;
};

const box: React.CSSProperties = {
  border: `1px solid ${T.line}`,
  borderRadius: 8,
  padding: "10px 12px",
  background: T.panel,
};

const btn: React.CSSProperties = {
  fontSize: 11.5,
  padding: "4px 10px",
  borderRadius: 6,
  border: `1px solid ${T.line}`,
  background: T.panel2,
  color: T.text2,
  cursor: "pointer",
};

export function KeyMemories(props: {
  storyId: string | null;
  storyTitle: string;
  setMsg: (s: string) => void;
}) {
  const { storyId, storyTitle, setMsg } = props;
  const [items, setItems] = useState<MemoryDto[]>([]);
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);

  const load = async () => {
    if (!storyId) {
      setItems([]);
      return;
    }
    try {
      setItems(await invoke<MemoryDto[]>("memory_list", { storyId }));
    } catch (e) {
      setMsg("⚠ 读取关键记忆失败：" + String(e));
    }
  };

  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [storyId]);

  const save = async (id: string) => {
    if (!storyId) return;
    setBusy(true);
    try {
      await invoke("memory_update", { storyId, id, body: draft });
      setEditing(null);
      setMsg("✅ 已保存关键记忆");
      await load();
    } catch (e) {
      setMsg("❌ 保存失败：" + String(e));
    }
    setBusy(false);
  };

  const remove = async (id: string) => {
    if (!storyId) return;
    if (!confirm("删除这条关键记忆？\n\n（只删这条文档；剧情本身不受影响）")) return;
    try {
      await invoke("memory_delete", { storyId, id });
      setMsg("🗑 已删除该条关键记忆");
      await load();
    } catch (e) {
      setMsg("❌ 删除失败：" + String(e));
    }
  };

  const exportAll = async () => {
    if (!storyId) return;
    setBusy(true);
    try {
      const [dir, n] = await invoke<[string, number]>("memory_export", {
        storyId,
        storyTitle: storyTitle || "未命名故事",
      });
      setMsg(n > 0 ? `✅ 已导出 ${n} 条关键记忆：${dir}` : `📂 已导出目录（这个分支还没有关键记忆）：${dir}`);
    } catch (e) {
      setMsg("❌ 导出失败：" + String(e));
    }
    setBusy(false);
  };

  return (
    <div style={box}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 8 }}>
        <div style={{ fontSize: 12.5, fontWeight: 650 }}>🧠 关键记忆（{items.length}）</div>
        <div style={{ flex: 1 }} />
        <button style={btn} onClick={() => void exportAll()} disabled={busy || !storyId}>
          导出（key-memories）
        </button>
      </div>
      <div style={{ fontSize: 11, color: T.text3, lineHeight: 1.7, marginBottom: 10 }}>
        系统从剧情里提炼：**表白 / 承诺 / 牺牲 / 背叛，每种只记第一次**（一个事件一个文档）。
        这里没有"新建"入口 —— 你可以改正文或删掉误记的条目。
      </div>

      {!storyId && <div style={{ fontSize: 11.5, color: T.text3 }}>先进入一个故事。</div>}
      {storyId && items.length === 0 && (
        <div style={{ fontSize: 11.5, color: T.text3 }}>
          还没有关键记忆。当剧情里第一次出现表白/承诺/牺牲/背叛时，这里会自动多一条。
        </div>
      )}

      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        {items.map((m) => (
          <div key={m.id} style={{ border: `1px solid ${T.line}`, borderRadius: 6, padding: "8px 10px", background: T.panel2 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 4 }}>
              <span style={{ fontSize: 12, fontWeight: 600 }}>{m.label}</span>
              <span style={{ fontSize: 11, color: T.text3 }}>{m.day_label}</span>
              <span style={{ fontSize: 10.5, color: T.text3 }}>
                重要度 {m.importance} · {m.branch ?? "主线"}
              </span>
              <div style={{ flex: 1 }} />
              {editing === m.id ? (
                <>
                  <button style={btn} onClick={() => void save(m.id)} disabled={busy}>保存</button>
                  <button style={btn} onClick={() => setEditing(null)}>取消</button>
                </>
              ) : (
                <>
                  <button style={btn} onClick={() => { setEditing(m.id); setDraft(memoryBodyView(m.body).text); }}>改</button>
                  <button style={btn} onClick={() => void remove(m.id)}>删</button>
                </>
              )}
            </div>
            {editing === m.id ? (
              <textarea
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                rows={5}
                style={{
                  width: "100%", fontSize: 11.5, lineHeight: 1.6, color: T.text2,
                  background: T.panel, border: `1px solid ${T.line}`, borderRadius: 6,
                  padding: 8, resize: "vertical",
                }}
              />
            ) : (
              (() => {
                // v3.5.1：正文是 Markdown —— 标题和上面已显示的「标签 · 日期」重复，引用块是系统备注，
                // 都拆出来分开显示（否则会看到 "# 第 4 天 · 表白" 和 ">" 符号）
                const v = memoryBodyView(m.body);
                return (
                  <>
                    {v.text && (
                      <div style={{ fontSize: 11.5, color: T.text2, lineHeight: 1.7, whiteSpace: "pre-wrap" }}>
                        {v.text}
                      </div>
                    )}
                    {v.note && (
                      <div style={{ fontSize: 10.5, color: T.text3, lineHeight: 1.6, marginTop: 4, opacity: 0.85 }}>
                        {v.note}
                      </div>
                    )}
                  </>
                );
              })()
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
