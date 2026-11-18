// 记忆卡片列表（2026-11-18）。
//
// ## 它现在是什么
//
// 原来它是**时间线页底部那个「🧠 关键记忆」面板**：自己 invoke 读列表、自己改正文、
// 自己删、自己导出。v3.16 把记忆的归属改成多参与者之后，这块搬进了角色详情卡的
// 「🧠 记忆」弹窗（`views/MemoryModal.tsx`）—— 于是这里只留"把一组卡画出来"：
// 读数据、分组、导出都在上层，**本文件不 invoke**（纯展示 + 三个回调）。
//
// ## 为什么还叫 KeyMemories.tsx
//
// `src-tauri/src/frontend_guard.rs` 用 `include_str!` 硬引用这个文件名，还要求它
// ≥120 行、必须含 `export` —— 改名会让 `cargo test` 直接编译失败。名字先留着，
// 语义以这个注释为准。
//
// ## 改 / 删 / 承诺徽标 为什么留着
//
// 记忆卡**会进提示词**（尾部【关键记忆】，1200 字预算）：错记一条"她答应了"就等于
// 每轮都在告诉模型她答应过。纯只读会把用户唯一的纠错出口推到文件系统。承诺徽标同理：
// 它是「待兑现」的界面结算路（另两条是模型协议、作者在消息里手打）。

import { useState } from "react";
import { memoryBodyView, sidesOf, type MemoryDto } from "../lib/memory";
import { renderRich } from "../lib/rich";
import { T } from "../theme";

/// 上层（弹窗）要提供三个写动作：改正文 / 删 / 轮转承诺状态
export type MemoryActions = {
  save: (id: string, body: string) => Promise<void>;
  remove: (m: MemoryDto) => Promise<void>;
  togglePromise: (m: MemoryDto) => Promise<void>;
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

/// 承诺状态 → 颜色（与弹窗顶部说明里的三态一致）
function promiseColor(state: string): string {
  return state === "已兑现" ? T.ok : state === "未兑现" ? T.danger : T.warn;
}

/// 承诺状态轮转：待兑现 → 已兑现 → 未兑现 → 待兑现（与旧面板一致）
export function nextPromiseState(state: string): string {
  return state === "待兑现" ? "已兑现" : state === "已兑现" ? "未兑现" : "待兑现";
}

export function MemoryCards(props: {
  items: MemoryDto[];
  actions: MemoryActions;
  busy: boolean;
}) {
  const { items, actions, busy } = props;
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");

  const save = async (id: string) => {
    try {
      await actions.save(id, draft);
      setEditing(null);
    } catch {
      // 上层已经提示过了；这里保留编辑态，别把用户刚打的字吞掉
    }
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      {items.map((m) => {
        // 2026-11-18：正文按角色名显示（行首 `[用户]/[AI]` → 人名；拿不到名字就保持原样）
        const view = memoryBodyView(m.body, sidesOf(m));
        const st = m.promise_state || "待兑现";
        return (
          <div key={m.id} style={{ border: `1px solid ${T.line}`, borderRadius: 6, padding: "8px 10px", background: T.panel2 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 4, flexWrap: "wrap" }}>
              <span style={{ fontSize: 12, fontWeight: 600 }}>{m.label}</span>
              {m.kind === "promise" && (
                <button
                  onClick={() => void actions.togglePromise(m)}
                  title={`承诺状态：${st}（点一下改成 ${nextPromiseState(st)}）`}
                  style={{ fontSize: 10, padding: "1px 6px", borderRadius: 6, border: `1px solid ${promiseColor(st)}`, background: "transparent", color: promiseColor(st), cursor: "pointer", fontFamily: "inherit" }}
                >
                  {st}
                </button>
              )}
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
                  <button style={btn} onClick={() => { setEditing(m.id); setDraft(view.text); }}>改</button>
                  <button className="btn-press" style={btn} onClick={() => void actions.remove(m)} disabled={busy}>删</button>
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
                  padding: 8, resize: "vertical", boxSizing: "border-box",
                }}
              />
            ) : (
              <>
                {view.text && (
                  <div style={{ fontSize: 11.5, color: T.text2, lineHeight: 1.7, whiteSpace: "pre-wrap" }}>
                    {/* 2026-11-18：正文里的 **加粗** 与 *斜体* 都渲染出来，别把星号直接露出来 */}
                    {renderRich(view.text)}
                  </div>
                )}
                {view.note && (
                  <div style={{ fontSize: 10.5, color: T.text3, lineHeight: 1.6, marginTop: 4, opacity: 0.85 }}>
                    {renderRich(view.note)}
                  </div>
                )}
              </>
            )}
          </div>
        );
      })}
    </div>
  );
}
