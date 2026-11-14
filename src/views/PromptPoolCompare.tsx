// 「双池对比」预览（2026-11-13 建立；2026-11-14 三次改版后定型）。
//
// 布局（用户定）：
//   ① 说明卡；② 操作行；③ **上半通用段**（两边一样，默认收起）；④ **有差异的段**两栏；
//   ⑤ **尾部**（2026-11-14 新增）：关系强度 / 角色属性 / 示例对话 / 关键记忆 ——
//      这四块也依赖视角，以前不在对比里，等于"记忆按角色"看不见。
//
// 只读：不调模型、不写库（后端 `preview_prompt_pair`）。

import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { T } from "../theme";
import { compareSegments, type Seg, type SegmentDiff } from "../lib/promptSegments";

type Side = { text: string; chars: number; tokens: number };
type Card = { label: string; day: string; kind: string; body: string };
type Group = { names: string[]; label: string; count: number; text: string; cards: Card[] };
type Pair = {
  ai_role: string;
  my_role: string;
  global: Side;
  role: Side;
  tail_global: Side;
  tail_role: Side;
  memory_groups: Group[];
  last_request: { id: number; node_id: number; tail_chars: number } | null;
};

const preStyle: React.CSSProperties = {
  margin: 0,
  padding: "8px 10px",
  maxHeight: 220,
  overflow: "auto",
  whiteSpace: "pre-wrap",
  overflowWrap: "anywhere",
  fontSize: 11.5,
  lineHeight: 1.65,
  color: T.text2,
};

export function PromptPoolCompare(props: { storyId?: number | null; aiRole?: string; myRole?: string; showIntro?: boolean }) {
  const { storyId, aiRole = "", myRole = "", showIntro = true } = props;
  const [data, setData] = useState<Pair | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [showCommon, setShowCommon] = useState(false);
  const [openCards, setOpenCards] = useState<string[]>([]);

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

  const diff = useMemo<SegmentDiff>(
    () => (data ? compareSegments(data.global.text, data.role.text) : { common: [], differing: [] }),
    [data],
  );
  const tailDiff = useMemo<SegmentDiff>(
    () => (data ? compareSegments(data.tail_global.text, data.tail_role.text) : { common: [], differing: [] }),
    [data],
  );
  const saved = data ? data.global.chars - data.role.chars : 0;
  // 漂移护栏（2026-11-14）：预览的尾部是"另抄一份"的选择逻辑，
  // 拿**最近一次真实请求**的 tail_block 字数当对照物；两边都对不上就红字提醒。
  const drift = (() => {
    if (!data?.last_request || data.last_request.tail_chars <= 0) return null;
    const real = data.last_request.tail_chars;
    const dGlobal = Math.abs(data.tail_global.chars - real);
    const dRole = Math.abs(data.tail_role.chars - real);
    const best = Math.min(dGlobal, dRole);
    const tol = Math.max(300, Math.round(real * 0.2));
    return { real, best, tol, ok: best <= tol, id: data.last_request.id };
  })();
  const tailSaved = data ? data.tail_global.chars - data.tail_role.chars : 0;

  const cell = (title: string, seg: Seg | null, tint: string) => (
    <div style={{ minWidth: 0, border: `1px solid ${T.line}`, borderRadius: 8, background: "#14171d", marginBottom: 10 }}>
      <div style={{ padding: "6px 10px", borderBottom: `1px solid ${T.line}`, fontSize: 11.5, color: tint, fontWeight: 650 }}>
        {title}
        <span style={{ marginLeft: 8, color: T.text3, fontWeight: 400, fontSize: 10.5 }}>
          {seg ? `${seg.body.length} 字` : "（没有这一段）"}
        </span>
      </div>
      <pre style={preStyle}>{seg ? seg.body : "—"}</pre>
    </div>
  );

  /** 有差异的段：两栏并排 */
  const differing = (d: SegmentDiff, emptyHint: string) =>
    d.differing.length === 0 ? (
      <div style={{ border: `1px dashed ${T.line}`, borderRadius: 8, padding: 10, fontSize: 11.5, color: T.text3 }}>
        {busy ? "拼装中…" : emptyHint}
      </div>
    ) : (
      d.differing.map((x) => (
        <div key={x.title} style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(320px, 1fr))", gap: 10, alignItems: "flex-start" }}>
          {cell(x.title, x.global, "#9fb4d8")}
          {cell(x.title, x.role, "#e3c07a")}
        </div>
      ))
    );

  /** 通用段：两边一样，只显示一次（可收起） */
  const common = (d: SegmentDiff, onOpen: () => void) =>
    d.common.length === 0 ? null : !showCommon ? (
      <button
        onClick={onOpen}
        title="展开通用段"
        style={{ display: "block", width: "100%", textAlign: "left", cursor: "pointer", border: `1px solid ${T.line}`, borderRadius: 8, background: "#12151a", padding: "8px 10px", marginBottom: 12, color: T.text3, fontSize: 11.5, fontFamily: "inherit" }}
      >
        ▸ 通用段（{d.common.length} 段，两个视角完全一样）：{d.common.map((s) => s.title).join(" · ")}
      </button>
    ) : (
      <div style={{ border: `1px solid ${T.line}`, borderRadius: 8, background: "#12151a", padding: 10, marginBottom: 12 }}>
        <div style={{ fontSize: 12, fontWeight: 650, color: T.text2, marginBottom: 8 }}>通用段（两个视角完全一样）</div>
        {d.common.map((s) => (
          <div key={s.title} style={{ borderTop: `1px solid ${T.lineSoft}`, paddingTop: 6, marginTop: 6 }}>
            <div style={{ fontSize: 11.5, color: "#9fb4d8", fontWeight: 650 }}>{s.title}</div>
            <pre style={preStyle}>{s.body}</pre>
          </div>
        ))}
      </div>
    );

  return (
    <div style={{ whiteSpace: "normal", minWidth: 0 }}>
      {showIntro && (
        <div style={{ border: `1px solid ${T.line}`, borderRadius: 8, background: T.panel2, padding: "10px 12px", marginBottom: 12 }}>
          <div style={{ fontSize: 13, fontWeight: 650, marginBottom: 6 }}>🔀 双池对比（全局视角 vs 角色视角）</div>
          <p style={{ margin: 0, fontSize: 11.5, color: T.text3, lineHeight: 1.8 }}>
            对比 <b>system 段</b>（排版规则 / 数据字典 / 世界 / 前史 / 角色 / 关系 / 情绪表演）与
            <b> 尾部的视角相关块</b>（关系强度 / 角色属性 / 示例对话），以及
            <b>关键记忆按关系组</b>的归属（a×b 与 b×a 同一组，有才显示）；
            其余尾部内容（当前模式、篇幅、约束、导演指令）每轮现拼，<b>不在这里比</b>。<b>只读</b>：不调模型、不写库。
          </p>
        </div>
      )}

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
            system 全局 {data.global.chars} → 角色 {data.role.chars} 字（{saved >= 0 ? "少" : "多"} {Math.abs(saved)}）｜
            尾部 全局 {data.tail_global.chars} → 角色 {data.tail_role.chars} 字（{tailSaved >= 0 ? "少" : "多"} {Math.abs(tailSaved)}）
          </span>
        ) : !storyId ? (
          <span style={{ fontSize: 11.5, color: T.text3 }}>先选一个故事</span>
        ) : null}
      </div>

      {err ? <div style={{ color: T.danger, fontSize: 12, marginBottom: 8 }}>❌ {err}</div> : null}

      {data && !data.ai_role ? (
        <div style={{ border: `1px solid ${T.line}`, background: "rgba(255,179,122,.08)", borderRadius: 8, padding: "8px 10px", marginBottom: 10, fontSize: 11.5, color: "#ffb37a", lineHeight: 1.7 }}>
          ⚠ 这个故事没记录扮演者（后端读到 ai_role=「{data.ai_role || "空"}」/ my_role=「{data.my_role || "空"}」），
          所以两块都一样。先在「对话」模式下发一轮，或在输入台选好「我扮演 / AI 扮演」再点「重新拼装」。
        </div>
      ) : null}

      {drift && !drift.ok ? (
        <div style={{ border: `1px solid ${T.line}`, background: "rgba(240,85,77,.08)", borderRadius: 8, padding: "8px 10px", marginBottom: 10, fontSize: 11.5, color: "#ff9d96", lineHeight: 1.7 }}>
          ⚠ 预览尾部与真实请求对不上：最近一次请求 #{drift.id} 的尾部是 {drift.real} 字，
          预览两版分别是 {data?.tail_global.chars} / {data?.tail_role.chars} 字（最接近也差 {drift.best} 字）。
          可能是「预览的选择逻辑」已经和真实装配漂移了 —— 改过「tail_view.rs」就该重跑一遍核对。
        </div>
      ) : null}

      {/* ③ 通用段（system 的） */}
      {common(diff, () => setShowCommon(true))}

      {/* ④ system 的有差异段 */}
      <div style={{ fontSize: 12, fontWeight: 650, color: T.text2, marginBottom: 8 }}>
        ① system 有差异的段（左：全局视角 ｜ 右：角色视角）
      </div>
      {differing(diff, "（没有差异段 —— 两栏完全一样）")}

      {/* ③ 关键记忆：按关系组（a×b 与 b×a 同一组） */}
      <div style={{ fontSize: 12, fontWeight: 650, color: T.text2, margin: "14px 0 8px" }}>
        ③ 关键记忆（按关系组：这一对之间的卡）
      </div>
      {(() => {
        const pairKey = [data?.ai_role ?? "", data?.my_role ?? ""]
          .map((s) => s.trim())
          .filter(Boolean)
          .sort();
        const mine = (data?.memory_groups ?? []).find(
          (g) => g.names.length === pairKey.length && g.names.slice().sort().join("|") === pairKey.join("|"),
        );
        const others = (data?.memory_groups ?? []).filter((g) => g !== mine);
        return (
          <>
            {mine ? (
              <div style={{ border: `1px solid ${T.line}`, borderRadius: 8, background: "#14171d", marginBottom: 10 }}>
                <div style={{ padding: "6px 10px", borderBottom: `1px solid ${T.line}`, fontSize: 11.5, fontWeight: 650, color: "#e3c07a" }}>
                  {mine.label}
                  <span style={{ marginLeft: 8, color: T.text3, fontWeight: 400, fontSize: 10.5 }}>{mine.count} 张</span>
                </div>
                <pre style={preStyle}>{mine.text.trim() || "（这一组没有可显示的卡片）"}</pre>
                {mine.cards.length > 0 ? (
                  <div style={{ borderTop: `1px solid ${T.lineSoft}`, padding: "6px 10px" }}>
                    <button
                      onClick={() =>
                        setOpenCards((prev) => (prev.includes(mine.label) ? prev.filter((x) => x !== mine.label) : [...prev, mine.label]))
                      }
                      style={{ height: 22, padding: "0 10px", borderRadius: 6, border: `1px solid ${T.line}`, background: "transparent", color: T.text3, cursor: "pointer", fontSize: 11, fontFamily: "inherit" }}
                    >
                      {openCards.includes(mine.label) ? "收起卡片正文" : `展开卡片正文（${mine.cards.length} 张，仅供查看，不进提示词）`}
                    </button>
                    {openCards.includes(mine.label)
                      ? mine.cards.map((c, i) => (
                          <div key={i} style={{ marginTop: 8, paddingTop: 6, borderTop: `1px dashed ${T.lineSoft}` }}>
                            <div style={{ fontSize: 11, color: T.text3, marginBottom: 4 }}>
                              {c.day} · {c.label}
                            </div>
                            <pre style={{ ...preStyle, maxHeight: 200, background: "#10131a", borderRadius: 6 }}>{c.body}</pre>
                          </div>
                        ))
                      : null}
                  </div>
                ) : null}
              </div>
            ) : (
              <div style={{ border: `1px dashed ${T.line}`, borderRadius: 8, padding: 10, fontSize: 11.5, color: T.text3, marginBottom: 10 }}>
                {pairKey.length === 2
                  ? `（${pairKey.slice().reverse().join(" × ")} 这一对还没有记忆卡 —— 有才显示）`
                  : "（先在输入台选好「我扮演 / AI 扮演」，才能按关系组看记忆）"}
              </div>
            )}
            {others.length > 0 ? (
              <div style={{ fontSize: 11, color: T.text3, marginBottom: 4 }}>
                其它关系组：{others.map((g) => `${g.label}（${g.count} 张）`).join(" · ")}
              </div>
            ) : null}
          </>
        );
      })()}

      {/* ⑤ 尾部（依赖视角的几块） */}
      <div style={{ fontSize: 12, fontWeight: 650, color: T.text2, margin: "14px 0 8px" }}>
        ② 尾部（依赖视角的几块：关系强度 / 角色属性 / 示例对话 / 关键记忆）
      </div>
      {differing(tailDiff, "（尾部没有差异 —— 这几块在两种视角下一样）")}
    </div>
  );
}
