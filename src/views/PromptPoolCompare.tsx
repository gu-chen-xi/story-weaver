// 「双池对比」预览（2026-11-13 建立；2026-11-14 两次改版后定型）。
//
// 布局（用户 2026-11-14 定）：
//   ① 说明卡；
//   ② **上：通用段**（两边一模一样的规则 —— 基础指令 / 输出排版 / 数据字典 / 台词口径 /
//      时间基准 / 前史 / 世界观 / 配合度 / 关系态势 / 情绪表演）；
//   ③ **下：两栏**（左全局 / 右角色），只放**真正有差异的段**（角色卡、关系、【你对面的他】…）。
//
// 只读：不调模型、不写库（后端 `preview_prompt_pair`）。

import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { T } from "../theme";
import { compareSegments, type Seg } from "../lib/promptSegments";

type Side = { text: string; chars: number; tokens: number };
type Pair = { ai_role: string; my_role: string; global: Side; role: Side };

const preStyle: React.CSSProperties = {
  margin: 0,
  padding: "8px 10px",
  maxHeight: 220,
  overflow: "auto",
  whiteSpace: "pre-wrap",
  wordBreak: "break-all",
  fontSize: 11.5,
  lineHeight: 1.65,
  color: T.text2,
};

export function PromptPoolCompare(props: { storyId?: number | null; aiRole?: string; myRole?: string }) {
  const { storyId, aiRole = "", myRole = "" } = props;
  const [data, setData] = useState<Pair | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [showCommon, setShowCommon] = useState(true);

  const load = async () => {
    if (!storyId) return;
    setBusy(true);
    setErr("");
    try {
      setData(await invoke<Pair>("preview_prompt_pair", {
        storyId: String(storyId),
        aiRole: aiRole || null,
        myRole: myRole || null,
      }));
    } catch (e) {
      setErr(String(e));
      setData(null);
    } finally {
      setBusy(false);
    }
  };
  useEffect(() => { void load(); }, [storyId, aiRole, myRole]);

  const diff = useMemo(
    () => (data ? compareSegments(data.global.text, data.role.text) : { common: [], differing: [] }),
    [data],
  );
  const saved = data ? data.global.chars - data.role.chars : 0;

  const cell = (title: string, seg: Seg | null, tint: string) => (
    <div style={{ flex: 1, minWidth: 0, border: `1px solid ${T.line}`, borderRadius: 8, background: "#14171d", marginBottom: 10 }}>
      <div style={{ padding: "6px 10px", borderBottom: `1px solid ${T.line}`, fontSize: 11.5, color: tint, fontWeight: 650 }}>
        {title}
        <span style={{ marginLeft: 8, color: T.text3, fontWeight: 400, fontSize: 10.5 }}>
          {seg ? `${seg.body.length} 字` : "（没有这一段）"}
        </span>
      </div>
      <pre style={preStyle}>{seg ? seg.body : "—"}</pre>
    </div>
  );

  return (
    <div>
      {/* ① 说明 */}
      <div style={{ border: `1px solid ${T.line}`, borderRadius: 8, background: T.panel2, padding: "10px 12px", marginBottom: 12 }}>
        <div style={{ fontSize: 13, fontWeight: 650, marginBottom: 6 }}>🔀 双池对比（全局视角 vs 角色视角）</div>
        <p style={{ margin: 0, fontSize: 11.5, color: T.text3, lineHeight: 1.8 }}>
          只对比 <b>system 段</b>（排版规则 / 数据字典 / 世界 / 前史 / 角色 / 关系）；尾部块（当前模式、数值与当下、关键记忆、
          示例对话、导演指令）每轮现拼，不在这里比。<b>只读</b>：不调模型、不写库。
        </p>
      </div>

      {/* 操作行 */}
      <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 10, flexWrap: "wrap" }}>
        <button onClick={() => void load()} disabled={busy || !storyId} style={{ height: 26, padding: "0 12px", borderRadius: 7, border: `1px solid ${T.line}`, background: T.panel2, color: T.text2, cursor: storyId ? "pointer" : "not-allowed", fontSize: 11.5, fontFamily: "inherit" }}>
          {busy ? "拼装中…" : "重新拼装"}
        </button>
        <button onClick={() => setShowCommon((v) => !v)} disabled={!data} style={{ height: 26, padding: "0 12px", borderRadius: 7, border: `1px solid ${T.line}`, background: "transparent", color: showCommon ? T.accent : T.text3, cursor: data ? "pointer" : "not-allowed", fontSize: 11.5, fontFamily: "inherit" }}>
          {showCommon ? "收起通用段" : "展开通用段"}
        </button>
        {data ? (
          <span style={{ fontSize: 11.5, color: T.text2 }}>
            {data.ai_role ? `${data.ai_role}${data.my_role ? ` × ${data.my_role}` : ""} ｜ ` : ""}
            全局 {data.global.chars} 字 → 角色 {data.role.chars} 字（{saved >= 0 ? "少" : "多"} {Math.abs(saved)} 字）｜
            通用 {diff.common.length} 段 / 有差异 {diff.differing.length} 段
          </span>
        ) : !storyId ? (
          <span style={{ fontSize: 11.5, color: T.text3 }}>先选一个故事</span>
        ) : null}
      </div>

      {err ? <div style={{ color: T.danger, fontSize: 12, marginBottom: 8 }}>❌ {err}</div> : null}

      {data && !data.ai_role ? (
        <div style={{ border: `1px solid ${T.line}`, background: "rgba(255,179,122,.08)", borderRadius: 8, padding: "8px 10px", marginBottom: 10, fontSize: 11.5, color: "#ffb37a", lineHeight: 1.7 }}>
          ⚠ 这个故事没记录扮演者（后端读到 ai_role=「{data.ai_role || "空"}」/ my_role=「{data.my_role || "空"}」），所以两栏一样、没有"有差异的段"。
          先在输入台的「对话」模式下发一轮（或在「我扮演 / AI 扮演」里选好人），再点「重新拼装」。
        </div>
      ) : null}

      {/* ② 上：通用段（两边一样，只显示一次） */}
      {showCommon && diff.common.length > 0 ? (
        <div style={{ border: `1px solid ${T.line}`, borderRadius: 8, background: "#12151a", padding: 10, marginBottom: 12 }}>
          <div style={{ fontSize: 12, fontWeight: 650, color: T.text2, marginBottom: 8 }}>
            通用段（两个视角完全一样）
            <span style={{ marginLeft: 8, fontWeight: 400, fontSize: 10.5, color: T.text3 }}>{diff.common.map((s) => s.title).join(" · ")}</span>
          </div>
          {diff.common.map((s) => (
            <div key={s.title} style={{ borderTop: `1px solid ${T.lineSoft}`, paddingTop: 6, marginTop: 6 }}>
              <div style={{ fontSize: 11.5, color: "#9fb4d8", fontWeight: 650 }}>{s.title}</div>
              <pre style={preStyle}>{s.body}</pre>
            </div>
          ))}
        </div>
      ) : null}

      {/* ③ 下：两栏，只放有差异的段 */}
      <div style={{ fontSize: 12, fontWeight: 650, color: T.text2, marginBottom: 8 }}>
        有差异的段（左：全局视角 ｜ 右：角色视角）
      </div>
      {diff.differing.length === 0 ? (
        <div style={{ border: `1px dashed ${T.line}`, borderRadius: 8, padding: 10, fontSize: 11.5, color: T.text3 }}>
          {busy ? "拼装中…" : "（没有差异段 —— 两栏完全一样）"}
        </div>
      ) : (
        diff.differing.map((d) => (
          <div key={d.title} style={{ display: "flex", gap: 10, alignItems: "flex-start" }}>
            {cell(d.title, d.global, "#9fb4d8")}
            {cell(d.title, d.role, "#e3c07a")}
          </div>
        ))
      )}
    </div>
  );
}
