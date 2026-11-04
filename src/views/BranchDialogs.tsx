// 分支相关的两个弹窗（2026-11-04 从 App.tsx 抽出，第三刀）。
//
// 为什么合在一个文件：它们是同一件事的两半 —— 新建（选分叉点）和改名/备注，
// 都只在「分支」这条线上用，拆两个文件反而更碎。
// 两者都在**根层**渲染（position: fixed 不能被祖先 overflow 裁掉）。

import type { BranchDto, TimelineNodeDto } from "../types";
import { T } from "../theme";
import { ForkNodePicker } from "./ForkNodePicker";
import { forkPickerGroups } from "../lib/branchView";

export type BranchCreateValue = { open: boolean; name: string; forkNodeId: number | null; busy: boolean };

export type CreateBranchDialogProps = {
  value: BranchCreateValue;
  onChange: (v: BranchCreateValue) => void;
  onCreate: () => void;
  onCancel: () => void;
  timelineNodes: TimelineNodeDto[];
  branches: BranchDto[];
  /** 复制到新分支的规模（角色 / 关系 / 世界观条数） */
  counts: { chars: number; relations: number; worlds: number };
};

export function CreateBranchDialog({ value, onChange, onCreate, onCancel, timelineNodes, branches, counts }: CreateBranchDialogProps) {
  if (!value.open) return null;
  return (
      <div style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,.64)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 950, padding: 20 }}>
        <div style={{ width: 460, maxWidth: "100%", background: T.win, border: `1px solid ${T.line}`, borderRadius: 12, padding: "16px 20px", boxShadow: "0 30px 80px rgba(0,0,0,.6)" }}>
          <div style={{ fontSize: 15, fontWeight: 650, marginBottom: 12 }}>新建故事分支</div>

          <div style={{ fontSize: 12, color: T.text3, marginBottom: 4 }}>分支名称</div>
          <input value={value.name} autoFocus
            onChange={(e) => onChange({ ...value, name: e.target.value })}
            onKeyDown={(e) => { if (e.key === "Enter") void onCreate(); }}
            placeholder="例如：角色A牺牲线"
            style={{ width: "100%", boxSizing: "border-box", padding: "7px 10px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontSize: 13, fontFamily: "inherit", marginBottom: 12 }} />

          <div style={{ fontSize: 12, color: T.text3, marginBottom: 4 }}>分叉节点（新分支从它的后面开始）</div>
          {/* 自绘下拉（2026-10-26）：原生 select 在这个位置会被系统判成「下方空间不够」而向上弹，
              把刚填的分支名挡住；用户要求固定向下展开 + 更紧凑的盒子。实现见 views/ForkNodePicker.tsx */}
          <div style={{ marginBottom: 12 }}>
            <ForkNodePicker
              groups={forkPickerGroups(timelineNodes, branches)}
              branches={branches}
              value={value.forkNodeId}
              onChange={(id) => onChange({ ...value, forkNodeId: id })}
            />
          </div>

          <div style={{ fontSize: 11.5, color: T.text2, lineHeight: 1.8, border: `1px solid ${T.lineSoft}`, borderRadius: 8, padding: "9px 11px", marginBottom: 12 }}>
            将**复制当前分支的世界状态**到新分支：角色 {counts.chars} · 关系 {counts.relations} · 世界观 {counts.worlds}
            <span style={{ color: T.text3 }}>
              <br />设定版本（永久）也会一并复制；临时版本不复制。
              <br />分叉点之前的对话节点是**共享**的，不复制。
            </span>
          </div>

          <div style={{ display: "flex", gap: 10, justifyContent: "flex-end" }}>
            <button onClick={() => onCancel()}
              style={{ fontSize: 12.5, padding: "7px 16px" }}>取消</button>
            <button disabled={value.busy || !value.name.trim() || value.forkNodeId == null}
              onClick={() => void onCreate()}
              style={{
                fontSize: 12.5, padding: "7px 18px", border: "none", borderRadius: 8, fontFamily: "inherit", fontWeight: 650,
                background: (value.busy || !value.name.trim()) ? T.panel3 : "linear-gradient(135deg,#FF7A18,#ff9a45)",
                color: (value.busy || !value.name.trim()) ? T.text3 : "#231505",
                cursor: (value.busy || !value.name.trim()) ? "not-allowed" : "pointer",
              }}>
              {value.busy ? "创建中…" : "创建并切换过去"}
            </button>
          </div>
        </div>
      </div>
  );
}

export type BranchEditValue = { id: string; name: string; note: string; busy: boolean };

export type BranchEditDialogProps = {
  edit: BranchEditValue | null;
  onChange: (v: BranchEditValue) => void;
  onSave: () => void;
  onCancel: () => void;
};

export function BranchEditDialog({ edit, onChange, onSave, onCancel }: BranchEditDialogProps) {
  if (!edit) return null;
  return (
      <div style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,.64)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 950, padding: 20 }}>
        <div style={{ width: 460, maxWidth: "100%", background: T.win, border: `1px solid ${T.line}`, borderRadius: 12, padding: "16px 20px", boxShadow: "0 30px 80px rgba(0,0,0,.6)" }}>
          <div style={{ fontSize: 15, fontWeight: 650, marginBottom: 12 }}>分支改名 / 备注</div>

          <div style={{ fontSize: 12, color: T.text3, marginBottom: 4 }}>分支名称</div>
          <input value={edit.name} autoFocus
            onChange={(e) => onChange({ ...edit, name: e.target.value })}
            onKeyDown={(e) => { if (e.key === "Enter") void onSave(); }}
            style={{ width: "100%", boxSizing: "border-box", padding: "7px 10px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontSize: 13, fontFamily: "inherit", marginBottom: 12 }} />

          <div style={{ fontSize: 12, color: T.text3, marginBottom: 4 }}>备注（一句话说明这条线是干什么的，可留空）</div>
          <textarea value={edit.note} rows={3}
            onChange={(e) => onChange({ ...edit, note: e.target.value })}
            placeholder="例如：假设夏婉琴在第 11 天没有回去，剧情会怎么走"
            style={{ width: "100%", boxSizing: "border-box", padding: "7px 10px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontSize: 13, fontFamily: "inherit", resize: "vertical", marginBottom: 12 }} />

          <div style={{ fontSize: 11.5, color: T.text3, lineHeight: 1.8, border: `1px solid ${T.lineSoft}`, borderRadius: 8, padding: "9px 11px", marginBottom: 12 }}>
            备注只在分支条与 🌲 树状图里显示（鼠标悬停可见），不会进入 AI 提示词。
          </div>

          <div style={{ display: "flex", gap: 10, justifyContent: "flex-end" }}>
            <button onClick={() => onCancel()} style={{ fontSize: 12.5, padding: "7px 16px" }}>取消</button>
            <button disabled={edit.busy || !edit.name.trim()} onClick={() => void onSave()}
              style={{
                fontSize: 12.5, padding: "7px 18px", border: "none", borderRadius: 8, fontFamily: "inherit", fontWeight: 650,
                background: (edit.busy || !edit.name.trim()) ? T.panel3 : "linear-gradient(135deg,#FF7A18,#ff9a45)",
                color: (edit.busy || !edit.name.trim()) ? T.text3 : "#231505",
                cursor: (edit.busy || !edit.name.trim()) ? "not-allowed" : "pointer",
              }}>
              {edit.busy ? "保存中…" : "保存"}
            </button>
          </div>
        </div>
      </div>
  );
}
