// 「新建分支 → 分叉节点」的自绘下拉（2026-10-26）。
//
// 为什么不用原生 <select>：
//   ① 这个弹窗在页面偏下，原生下拉会被系统判成"空间不够"而**向上弹**，把刚填的分支名挡住；
//      用户明确要「向下展开」—— 原生控件给不了这个保证，只能自绘。
//   ② 原生 select 的盒子高度跟随系统行高（约 34px），用户要求更紧凑的盒子。
//   ③ 分叉节点是按分支分组的（`forkPickerGroups`），自绘才能把组名 + 分支色点画成标题行。
//
// 交互：点开 / 点外面或 Esc 关 / 点某项选中并关闭 / 打开时把选中项滚进可视区。
// 只做"选一个 id"，不持有业务状态 —— 状态仍在 App 的 `branchDialog.forkNodeId`。

import { useEffect, useRef, useState } from "react";
import { T } from "../theme";
import { branchColor } from "../lib/branchView";
import type { BranchDto, TimelineNodeDto } from "../types";

const TRIGGER_H = 28;          // 触发盒高度（原生 select 约 34，这里更紧凑）
const LIST_MAX_H = 220;        // 展开列表最大高度

export type ForkGroup = { key: string; name: string; nodes: TimelineNodeDto[] };

type Props = {
  groups: ForkGroup[];
  branches: BranchDto[];
  value: number | null;
  onChange: (id: number) => void;
};

export function ForkNodePicker({ groups, branches, value, onChange }: Props) {
  const [open, setOpen] = useState(false);
  const boxRef = useRef<HTMLDivElement | null>(null);
  const listRef = useRef<HTMLDivElement | null>(null);

  const current = groups.flatMap((g) => g.nodes).find((n) => n.id === value) ?? null;
  const labelOf = (n: TimelineNodeDto) =>
    `${n.node_type === "interlude" ? "🎭 " : ""}${n.date_label}`;

  // 打开时把选中项滚到列表可视区（天多的时候默认停在顶上，看不见自己选的那天）
  useEffect(() => {
    if (!open) return;
    const list = listRef.current;
    if (!list) return;
    const sel = list.querySelector<HTMLElement>('[data-selected="1"]');
    if (sel) sel.scrollIntoView({ block: "nearest" });
  }, [open]);

  // Esc 关闭（弹窗本身也有 Esc，这里先吃掉一层）
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") { e.stopPropagation(); setOpen(false); } };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [open]);

  return (
    <div ref={boxRef} style={{ position: "relative" }}>
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        title="选择从哪一天之后开始新分支"
        style={{
          width: "100%", height: TRIGGER_H, boxSizing: "border-box",
          display: "flex", alignItems: "center", gap: 8,
          padding: "0 10px", borderRadius: 8,
          border: `1px solid ${open ? T.accent : T.line}`,
          background: "#191c22", color: current ? T.text1 : T.text3,
          fontSize: 13, fontFamily: "inherit", cursor: "pointer", textAlign: "left",
        }}
      >
        <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
          {current ? labelOf(current) : "选择分叉节点"}
        </span>
        <span style={{ color: T.text3, fontSize: 10, transform: open ? "rotate(180deg)" : "none", transition: "transform .12s" }}>▾</span>
      </button>

      {open && (
        <>
          {/* 点遮罩关闭：铺满窗口、在列表之下 —— 不用 document 监听，避免和弹窗的点击打架 */}
          <div onClick={() => setOpen(false)} style={{ position: "fixed", inset: 0, zIndex: 959 }} />
          <div
            ref={listRef}
            style={{
              position: "absolute", left: 0, right: 0, top: `calc(100% + 4px)`, zIndex: 960,
              maxHeight: LIST_MAX_H, overflowY: "auto",
              background: T.panel2, border: `1px solid ${T.line}`, borderRadius: 8,
              boxShadow: "0 18px 44px rgba(0,0,0,.55)", padding: "4px 0",
            }}
          >
            {groups.map((g) => (
              <div key={g.key}>
                <div style={{
                  display: "flex", alignItems: "center", gap: 6,
                  padding: "5px 10px 3px", fontSize: 10.5, color: T.text3, letterSpacing: 0.3,
                }}>
                  <span style={{ width: 6, height: 6, borderRadius: 3, background: branchColor(branches, g.key === "main" ? null : g.key), flexShrink: 0 }} />
                  {g.name}
                  <span style={{ marginLeft: "auto", fontSize: 9.5, opacity: 0.7 }}>{g.nodes.length} 天</span>
                </div>
                {g.nodes.map((n) => {
                  const on = n.id === value;
                  return (
                    <div
                      key={n.id}
                      data-selected={on ? "1" : "0"}
                      onClick={() => { onChange(n.id); setOpen(false); }}
                      style={{
                        padding: "4px 10px 4px 22px", fontSize: 12.5, cursor: "pointer",
                        color: on ? T.accent : T.text2,
                        background: on ? T.accentSoft : "transparent",
                      }}
                      onMouseEnter={(e) => { if (!on) e.currentTarget.style.background = "rgba(255,255,255,.05)"; }}
                      onMouseLeave={(e) => { if (!on) e.currentTarget.style.background = "transparent"; }}
                    >
                      {on ? "✓ " : ""}{labelOf(n)}
                    </div>
                  );
                })}
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
