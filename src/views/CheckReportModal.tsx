// 「检查报告」弹窗（C2，2026-10-18 从 App.tsx 抽出）。
//
// 触发：剧情页点「🔍 检查」→ 后端跑结构/内容检查 → 这里列出问题 + 可执行的补丁。
// 纯展示 + 回调：所有状态与动作仍由 App 提供（checkBoard / checkItems / applyCheckPatch …）。

import { T } from "../theme";
import type { Dispatch, SetStateAction } from "react";
import type { Character, CheckItemT, PatchItemT, Relation, WorldSetting } from "../types";

export type CheckReportModalProps = {
  /// 名称反查用（把 id / 关系名显示成人看得懂的名字）
  chars: Character[];
  relations: Relation[];
  worlds: WorldSetting[];
  checkBoard: "characters" | "world" | null;
  checkBusy: boolean;
  checkInput: string;
  setCheckInput: Dispatch<SetStateAction<string>>;
  checkItems: CheckItemT[];
  checkNote: string;
  checkPatch: PatchItemT[];
  checkPick: Set<number>;
  setCheckPick: Dispatch<SetStateAction<Set<number>>>;
  setCheckBoard: Dispatch<SetStateAction<"characters" | "world" | null>>;
  applyCheckPatch: () => Promise<void>;
  continueCheck: () => Promise<void>;
};

export function CheckReportModal(props: CheckReportModalProps) {
  const { chars, relations, worlds, checkBoard, checkBusy, checkInput, setCheckInput, checkItems, checkNote, checkPatch, checkPick, setCheckPick, setCheckBoard, applyCheckPatch, continueCheck } = props;
      const noIssue = !checkBusy && checkItems.length === 0 && checkPatch.length === 0;
      // 字段名映射按板块区分（避免角色板块的补丁显示成「规模 / 对主角态度」这类世界字段名）
      const CHAR_FIELDS: Record<string, string> = {
        name: "角色名", alias: "别名", role: "定位", gender: "性别", age: "年龄", one_liner: "一句话身份",
        profile: "人物小传", speech_style: "说话方式", traits: "性格",
      };
      const REL_FIELDS: Record<string, string> = {
        relation_type: "关系类型", strength: "关系数值", state: "关系状态", description: "关系说明", direction: "关系方向",
      };
      const WORLD_FIELDS: Record<string, string> = {
        key: "名称", value: "内容", category: "分类", attitude: "对主角态度", scale: "规模",
        leader: "头目", control_range: "控制范围", map_x: "地图 X", map_y: "地图 Y",
      };
      // 映射表以 **patch 自带的 board** 为准（不靠 target_kind 猜，避免模型标错类型时显示成别的板块字段）
      // 字段名与"角色/关系/条目"后缀**由字段归属直接推断** —— 这样即使模型把 board / target_kind 写错，
      // 界面也不会显示成别的板块的字段（此前出现过角色补丁显示成「条目 · 内容」的情况）。
      const fieldOwned = (f: string): "char" | "rel" | "world" | null =>
        (f in CHAR_FIELDS) ? "char" : (f in REL_FIELDS) ? "rel" : (f in WORLD_FIELDS) ? "world" : null;
      const fieldLabel = (pp: { board?: string; target_kind: string; field: string }) => {
        const owner = fieldOwned(pp.field);
        if (owner === "char") return CHAR_FIELDS[pp.field];
        if (owner === "rel") return REL_FIELDS[pp.field];
        if (owner === "world") return WORLD_FIELDS[pp.field];
        return `未知字段「${pp.field}」`;
      };
      const targetNoun = (pp: { target_kind: string; field: string }) =>
        fieldOwned(pp.field) === "rel" ? "关系" : fieldOwned(pp.field) === "char" ? "角色" : "条目";
      const kindColor = (k: string) => k === "空值" ? T.warn : k === "重复" || k === "悬空" || k === "坐标冲突" || k === "数值非法" ? T.danger : "#8ab4ff";
      const before = (p: { target_kind: string; target_name: string; field: string }) => {
        try {
          if (p.target_kind === "character") {
            const c = chars.find((x) => x.name === p.target_name);
            return c ? String((c as unknown as Record<string, string>)[p.field] ?? "") : "（找不到该角色）";
          }
          if (p.target_kind === "relation") {
            const [f, tt] = p.target_name.split(/→|->/).map((s) => s.trim());
            const a = chars.find((x) => x.name === f), b = chars.find((x) => x.name === tt);
            const r = a && b ? relations.find((x) => x.from_character_id === a.id && x.to_character_id === b.id) : undefined;
            return r ? String((r as unknown as Record<string, string>)[p.field] ?? "") : "（找不到该关系）";
          }
          const w = worlds.find((x) => x.key === p.target_name);
          return w ? String((w as unknown as Record<string, string>)[p.field] ?? "") : "（找不到该条目）";
        } catch { return ""; }
      };
      const cut = (s: string, n = 90) => s.length > n ? s.slice(0, n) + "…" : s;
      return (
        <div style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,.55)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 61 }}
          onClick={() => { if (noIssue) setCheckBoard(null); }}>
          <div onClick={(e) => e.stopPropagation()} style={{ width: noIssue ? 360 : 720, maxWidth: "92vw", maxHeight: "86vh", overflowY: "auto", background: T.panel, border: `1px solid ${T.line}`, borderRadius: 14, padding: "18px 20px" }}>
            <div style={{ fontSize: 14, fontWeight: 650, marginBottom: 4 }}>
              🔍 检查 · {checkBoard === "characters" ? "角色管理" : "世界设定"}
            </div>

            {/* 1) 没问题：点任意处关闭 */}
            {noIssue ? (
              <div style={{ textAlign: "center", padding: "18px 0 6px" }}>
                <div style={{ fontSize: 30, marginBottom: 8 }}>✅</div>
                <div style={{ fontSize: 13, color: T.ok, fontWeight: 650 }}>没发现问题</div>
                <div style={{ fontSize: 11.5, color: T.text3, marginTop: 6, lineHeight: 1.8 }}>
                  本地规则与 AI 都没挑出读不通、有歧义或与数据字典矛盾的地方。<br />（点击任意处关闭）
                </div>
              </div>
            ) : (
              <>
                {/* 2) 检查中 */}
                {checkBusy && (
                  <div style={{ fontSize: 12, color: T.text3, padding: "10px 0" }}>正在检查…（本地规则即时出结果；AI 检查需几秒）</div>
                )}

                {/* 3) 问题清单 */}
                {checkItems.length > 0 && (
                  <div style={{ border: `1px solid ${T.line}`, borderRadius: 10, overflow: "hidden", marginTop: 8 }}>
                    <div style={{ display: "grid", gridTemplateColumns: "150px 1fr", gap: 0, fontSize: 10.5, color: T.text3, background: T.panel2, padding: "7px 12px" }}>
                      <span>位置 / 字段</span><span>问题与建议</span>
                    </div>
                    {checkItems.map((it, i) => (
                      <div key={i} style={{ display: "grid", gridTemplateColumns: "150px 1fr", gap: 0, borderTop: `1px solid ${T.lineSoft}`, padding: "9px 12px", fontSize: 11.5 }}>
                        <div>
                          <div style={{ color: T.text1, fontWeight: 600, overflow: "hidden", textOverflow: "ellipsis" }}>{it.where_ || "（未指明）"}</div>
                          <div style={{ color: T.text3, fontSize: 10.5, marginTop: 2 }}>{it.field || "—"}</div>
                          <div style={{ display: "inline-block", marginTop: 4, fontSize: 9.5, padding: "1px 6px", borderRadius: 5, color: kindColor(it.kind), background: `${kindColor(it.kind)}22` }}>{it.kind || "问题"}</div>
                        </div>
                        <div style={{ color: T.text2, lineHeight: 1.7 }}>
                          <div>{it.why}</div>
                          {it.suggestion && <div style={{ color: T.ok, marginTop: 3 }}>建议：{it.suggestion}</div>}
                        </div>
                      </div>
                    ))}
                  </div>
                )}

                {/* 4) 可执行的补丁（AI 给出的结构化修改） */}
                {checkPatch.length > 0 && (
                  <div style={{ marginTop: 12 }}>
                    <div style={{ fontSize: 12, fontWeight: 650, marginBottom: 6, display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
                      <span>可一键替换 {checkPatch.length} 条（已勾选 {checkPick.size} 条；执行前会二次确认并存一个设定版本）</span>
                      <button onClick={() => setCheckPick(new Set(checkPatch.map((_, i) => i)))}
                        style={{ fontSize: 10.5, padding: "2px 9px", borderRadius: 6, border: `1px solid ${T.line}`, background: "transparent", color: T.text3, cursor: "pointer" }}>全选</button>
                      <button onClick={() => setCheckPick(new Set())}
                        style={{ fontSize: 10.5, padding: "2px 9px", borderRadius: 6, border: `1px solid ${T.line}`, background: "transparent", color: T.text3, cursor: "pointer" }}>全不选</button>
                    </div>
                    <div style={{ border: `1px solid ${T.line}`, borderRadius: 10, overflow: "hidden" }}>
                      {checkPatch.map((pp, i) => (
                        <div key={i} style={{ borderTop: i ? `1px solid ${T.lineSoft}` : "none", padding: "9px 12px", fontSize: 11, opacity: checkPick.has(i) ? 1 : 0.45, display: "flex", gap: 9, alignItems: "flex-start" }}>
                          <input type="checkbox" checked={checkPick.has(i)}
                            onChange={(e) => setCheckPick((prev) => { const nx = new Set(prev); if (e.target.checked) nx.add(i); else nx.delete(i); return nx; })}
                            style={{ marginTop: 2, cursor: "pointer", accentColor: T.accent }} />
                          <div style={{ flex: 1, minWidth: 0 }}>
                          <div style={{ color: T.text1, fontWeight: 600 }}>
                            {targetNoun(pp)}「{pp.target_name}」· {fieldLabel(pp)}
                          </div>
                          <div style={{ marginTop: 4, lineHeight: 1.7 }}>
                            <div style={{ color: T.danger }}>− {cut((pp.before && pp.before.trim()) ? pp.before : before(pp)) || "（空）"}</div>
                            <div style={{ color: T.ok }}>＋ {cut(pp.value) || "（空）"}</div>
                          </div>
                          </div>
                        </div>
                      ))}
                    </div>
                  </div>
                )}

                {/* 4b) 有问题但没有可执行补丁 → 明说原因 */}
                {checkPatch.length === 0 && checkItems.length > 0 && (
                  <div style={{ marginTop: 10, padding: "8px 10px", borderRadius: 8, background: "rgba(255,122,24,.07)", border: "1px solid rgba(255,122,24,.25)", fontSize: 11, color: T.text2, lineHeight: 1.7 }}>
                    本次没有可自动替换的项 —— 上面这些问题需要你在对应页面自己改（例如<b>关系方向</b>这类字段不在自动替换范围内），或点「继续对话」补充说明后让 AI 再给一版建议。
                  </div>
                )}

                {/* 4c) 补丁里出现未知字段 → 提前警告（这批会被软件拒绝，不会撞数据库） */}
                {checkPatch.some((pp) => fieldLabel(pp).startsWith("未知字段")) && (
                  <div style={{ marginTop: 10, padding: "8px 10px", borderRadius: 8, background: "rgba(240,85,77,.08)", border: "1px solid rgba(240,85,77,.3)", fontSize: 11, color: "#ff8c85", lineHeight: 1.7 }}>
                    ⚠ 有补丁使用了本板块不存在的字段（上面标着「未知字段」的那些）—— 这类条目**点了也不会执行**，
                    软件会在替换时报出原因。建议先点「继续对话」让 AI 重新给一版，或取消勾选它们。
                  </div>
                )}

                {/* 5) 「继续对话」输入 */}
                <div style={{ marginTop: 12 }}>
                  <label style={{ fontSize: 11.5, color: T.text2 }}>继续对话（可选）—— 说明你的想法，让 AI 更新建议</label>
                  <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
                    <input value={checkInput} onChange={(e) => setCheckInput(e.target.value)}
                      onKeyDown={(e) => { if (e.key === "Enter") continueCheck(); }}
                      placeholder="例如：夏婉琴是女孩子，不要用“他”指代"
                      style={{ flex: 1, padding: "8px 11px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontSize: 12.5, outline: "none" }} />
                    <button onClick={continueCheck} disabled={checkBusy} style={{ height: 34, padding: "0 14px", borderRadius: 8, border: `1px solid ${T.line}`, background: T.panel2, color: T.text2, cursor: "pointer", fontSize: 12 }}>继续对话</button>
                  </div>
                </div>
              </>
            )}

            {checkNote && (
              <div style={{ marginTop: 10, padding: "7px 10px", borderRadius: 8, background: "rgba(240,85,77,.08)", border: "1px solid rgba(240,85,77,.3)", fontSize: 11, color: "#ff8c85", lineHeight: 1.7 }}>
                {checkNote}
              </div>
            )}

            {/* 底部按钮 */}
            {!noIssue && (
              <div style={{ display: "flex", gap: 8, marginTop: 16, justifyContent: "flex-end", alignItems: "center" }}>
                {checkBusy && <span style={{ fontSize: 11, color: T.text3, marginRight: "auto" }}>处理中…</span>}
                <button onClick={() => setCheckBoard(null)} style={{ height: 32, padding: "0 14px", borderRadius: 8, border: `1px solid ${T.line}`, background: T.panel2, color: T.text2, cursor: "pointer", fontSize: 12 }}>取消</button>
                <button onClick={applyCheckPatch} disabled={checkBusy || checkPick.size === 0}
                  title={checkPick.size === 0 ? "请先勾选要替换的条目" : "执行前会二次确认"}
                  style={{ height: 32, padding: "0 16px", borderRadius: 8, border: "none", background: checkPick.size === 0 ? T.panel3 : "linear-gradient(135deg,#FF7A18,#ff9a45)", color: checkPick.size === 0 ? T.text3 : "#231505", cursor: checkPick.size === 0 ? "not-allowed" : "pointer", fontSize: 12, fontWeight: 650 }}>
                  一键替换{checkPick.size > 0 ? `（${checkPick.size}）` : ""}
                </button>
              </div>
            )}
          </div>
        </div>
      );
}
