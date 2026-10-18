// 「插入事件」弹窗（C2，2026-10-18 从 App.tsx 抽出）。
//
// 触发：时间线里「时间流逝·第 X~Y 天（无事件）」处的「+ 在此插入事件」。
// 规则：天数必须在空档区间内、且该天还不存在（提交按钮会实时校验并禁用）。

import { T } from "../theme";
import type { Dispatch, SetStateAction } from "react";
import type { TimelineNodeDto } from "../types";

export type GapPrompt = { from: number; to: number; day: string; title: string };

export type InsertEventModalProps = {
  gapPrompt: GapPrompt;
  setGapPrompt: Dispatch<SetStateAction<GapPrompt | null>>;
  dayNodesSorted: () => { node: TimelineNodeDto; dayNo: number }[];
  insertEventAtDay: () => Promise<void>;
};

export function InsertEventModal(props: InsertEventModalProps) {
  const { gapPrompt, setGapPrompt, dayNodesSorted, insertEventAtDay } = props;
    return (
      <div style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,.55)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 60 }} onClick={() => setGapPrompt(null)}>
        <div onClick={(e) => e.stopPropagation()} style={{ width: 420, background: T.panel, border: `1px solid ${T.line}`, borderRadius: 14, padding: "18px 20px" }}>
          <div style={{ fontSize: 14, fontWeight: 650, marginBottom: 4 }}>⏳ 在这段里插入事件</div>
          <div style={{ fontSize: 11.5, color: T.text3, marginBottom: 12, lineHeight: 1.7 }}>
            当前空白期：<b style={{ color: T.text2 }}>第 {gapPrompt.from} ~ {gapPrompt.to} 天</b>（这段时间没有剧情）<br />
            插入后会在这段里新建一个天数节点；不插入也完全不影响现有内容。
          </div>
          {/* 规则说明 + 即时校验：早先只在提交后才报错，用户根本不知道有这两条限制（用户反馈）。
              两条规则：① 只能填**区间内**的天；② **已经存在的那一天不能再建**（否则会出现两个「第 N 天」）。 */}
          {(() => {
            const dn = Number(gapPrompt.day);
            const inRange = dn >= gapPrompt.from && dn <= gapPrompt.to;
            const exists = dayNodesSorted().some((x) => x.dayNo === dn);
            const bad = !dn ? "请填写天数"
              : (!Number.isInteger(dn) || dn <= 0) ? "天数必须是正整数"
              : !inRange ? `只能填区间内的天数（第 ${gapPrompt.from} ~ ${gapPrompt.to} 天）`
              : exists ? `第 ${dn} 天已经存在，不能重复创建`
              : "";
            return (
              <div style={{ fontSize: 11, lineHeight: 1.8, padding: "7px 10px", borderRadius: 8, marginBottom: 10,
                border: `1px solid ${bad ? T.danger + "55" : T.line}`,
                background: bad ? "rgba(240,85,77,.08)" : T.panel2,
                color: bad ? T.danger : T.text3 }}>
                <div style={{ fontWeight: 600, color: bad ? T.danger : T.text2, marginBottom: 2 }}>
                  {bad ? "⚠ " + bad : "✅ 可以插入"}
                </div>
                规则：① 只能选<b>这段空白期内</b>的天数（第 {gapPrompt.from} ~ {gapPrompt.to} 天）；
                ② <b>已经存在的天不能重复创建</b>（避免出现两个「第 N 天」）。
              </div>
            );
          })()}
          <label style={{ fontSize: 11.5, color: T.text2 }}>插入到第几天</label>
          <input value={gapPrompt.day} onChange={(e) => setGapPrompt({ ...gapPrompt, day: e.target.value.replace(/[^0-9]/g, "") })}
            placeholder={`${gapPrompt.from} ~ ${gapPrompt.to}`}
            style={{ width: "100%", boxSizing: "border-box", marginTop: 4, padding: "8px 11px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontFamily: "monospace", fontSize: 12.5, outline: "none" }} />
          <label style={{ fontSize: 11.5, color: T.text2, display: "block", marginTop: 10 }}>事件标题（可选，仅用于你自己备注）</label>
          <input value={gapPrompt.title} onChange={(e) => setGapPrompt({ ...gapPrompt, title: e.target.value })}
            placeholder="例如：宸熙出差三天"
            style={{ width: "100%", boxSizing: "border-box", marginTop: 4, padding: "8px 11px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontSize: 12.5, outline: "none" }} />
          <div style={{ display: "flex", gap: 8, marginTop: 14, justifyContent: "flex-end" }}>
            <button onClick={() => setGapPrompt(null)} style={{ height: 30, padding: "0 14px", borderRadius: 8, border: `1px solid ${T.line}`, background: T.panel2, color: T.text2, cursor: "pointer", fontSize: 12 }}>取消</button>
            {(() => {
              const dn = Number(gapPrompt.day);
              const ok = Number.isInteger(dn) && dn > 0
                && dn >= gapPrompt.from && dn <= gapPrompt.to
                && !dayNodesSorted().some((x) => x.dayNo === dn);
              return (
                <button onClick={insertEventAtDay} disabled={!ok}
                  title={ok ? "在这天插入一个事件节点" : "天数不合法或该天已存在，见上方提示"}
                  style={{ height: 30, padding: "0 14px", borderRadius: 8, border: "none",
                    background: ok ? "linear-gradient(135deg,#FF7A18,#ff9a45)" : T.line,
                    color: ok ? "#231505" : T.text3,
                    cursor: ok ? "pointer" : "not-allowed", fontSize: 12, fontWeight: 650 }}>
                  插入事件节点
                </button>
              );
            })()}
          </div>
        </div>
      </div>
)
}
