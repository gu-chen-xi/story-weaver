// 「回滚设定版本」的差异预览弹窗（2026-11-04 从 App.tsx 抽出，App.tsx 第一刀）。
//
// 为什么单独一个文件：回滚是破坏性操作，这个弹窗负责"先看清会失去什么"，
// 里面有 diff 渲染 + 三段分组 + 一句"不可撤销"的警告 —— 它和 App 的其它状态零耦合，
// 只是在根层渲染（position: fixed 不能被祖先 overflow 裁掉）。

import type { SnapDiff, SnapItemChange } from "../types";
import { T } from "../theme";

export type SnapPreview = { version: number; diff: SnapDiff; loading: boolean; error: string };

export type SnapshotPreviewModalProps = {
  preview: SnapPreview;
  onClose: () => void;
  onConfirm: (version: number) => void;
};

export function SnapshotPreviewModal({ preview, onClose, onConfirm }: SnapshotPreviewModalProps) {
  return (
      <div style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,.66)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 900, padding: 20 }}>
        <div style={{ width: 720, maxWidth: "100%", maxHeight: "86vh", overflowY: "auto", background: T.win, border: `1px solid ${T.line}`, borderRadius: 12, padding: "16px 20px", boxShadow: "0 30px 80px rgba(0,0,0,.6)" }}>
          <div style={{ fontSize: 15, fontWeight: 650, marginBottom: 4 }}>
            ⚠ 回滚到设定版本 V{preview.version} —— 先看清会发生什么
          </div>
          <div style={{ fontSize: 11.5, color: T.text3, lineHeight: 1.75, marginBottom: 12 }}>
            回滚会用该**设定版本**整表替换当前的角色 / 关系 / 世界观。<b style={{ color: T.warn }}>这是不可撤销的</b>（回滚本身也会存成一个新的设定版本）。
            下面列出「当前 → 回滚后」的差异：
          </div>

          {preview.loading && <div style={{ fontSize: 12.5, color: T.text3, padding: "18px 0" }}>正在对比…</div>}
          {!!preview.error && (
            <div style={{ fontSize: 12.5, color: T.danger, padding: "10px 12px", border: `1px solid ${T.danger}`, borderRadius: 8, marginBottom: 12 }}>
              对比失败：{preview.error}
              <div style={{ fontSize: 11, color: T.text3, marginTop: 6 }}>仍然可以回滚，但建议先查清原因。</div>
            </div>
          )}

          {!preview.loading && (() => {
            const d = preview.diff;
            const total = d.characters.length + d.relations.length + d.world_settings.length;
            const KIND: Record<string, { t: string; c: string }> = {
              added: { t: "将新增", c: T.ok },
              removed: { t: "将删除", c: T.danger },
              modified: { t: "将修改", c: T.warn },
            };
            const Section = ({ title, items }: { title: string; items: SnapItemChange[] }) => items.length === 0 ? null : (
              <div style={{ marginBottom: 14 }}>
                <div style={{ fontSize: 12.5, fontWeight: 650, marginBottom: 6 }}>{title}（{items.length}）</div>
                <div style={{ display: "flex", flexDirection: "column", gap: 5 }}>
                  {items.map((it, i) => (
                    <div key={i} style={{ border: `1px solid ${T.lineSoft}`, borderRadius: 8, padding: "7px 10px", background: T.panel }}>
                      <div style={{ fontSize: 12 }}>
                        <span style={{ color: KIND[it.kind]?.c ?? T.text2, fontWeight: 650 }}>{KIND[it.kind]?.t ?? it.kind}</span>
                        <span style={{ marginLeft: 8, color: T.text1 }}>{it.label}</span>
                      </div>
                      {it.fields.length > 0 && (
                        <div style={{ marginTop: 5, paddingLeft: 2 }}>
                          {it.fields.map((f, j) => (
                            <div key={j} style={{ fontSize: 11, color: T.text3, lineHeight: 1.75 }}>
                              {f.field}：
                              <span style={{ color: T.text2 }}>{f.before || "（空）"}</span>
                              <span style={{ margin: "0 6px", color: T.accent }}>→</span>
                              <span style={{ color: T.text1 }}>{f.after || "（空）"}</span>
                            </div>
                          ))}
                        </div>
                      )}
                    </div>
                  ))}
                </div>
              </div>
            );
            if (total === 0) {
              return (
                <div style={{ fontSize: 12.5, color: T.text3, border: `1px dashed ${T.line}`, borderRadius: 8, padding: "14px 12px", marginBottom: 12 }}>
                  当前设定与该版本**完全一致**，回滚不会产生任何变化。
                </div>
              );
            }
            return (
              <>
                <div style={{ fontSize: 12, color: T.text2, marginBottom: 10 }}>
                  共 <b style={{ color: T.accent }}>{total}</b> 处不同：
                  新增 {[...d.characters, ...d.relations, ...d.world_settings].filter((x) => x.kind === "added").length} ·
                  修改 {[...d.characters, ...d.relations, ...d.world_settings].filter((x) => x.kind === "modified").length} ·
                  删除 {[...d.characters, ...d.relations, ...d.world_settings].filter((x) => x.kind === "removed").length}
                </div>
                <Section title="角色卡" items={d.characters} />
                <Section title="关系" items={d.relations} />
                <Section title="世界观 / 设定" items={d.world_settings} />
              </>
            );
          })()}

          <div style={{ display: "flex", gap: 10, justifyContent: "flex-end", marginTop: 16 }}>
            <button onClick={() => onClose()} style={{ fontSize: 12.5, padding: "7px 16px" }}>取消</button>
            <button className="btn-press"
              disabled={preview.loading || (() => { const d = preview.diff; return d.characters.length + d.relations.length + d.world_settings.length === 0; })()}
              onClick={() => void onConfirm(preview.version)}
              title="整表替换设定，不可撤销"
              style={{
                fontSize: 12.5, padding: "7px 18px", border: "none", borderRadius: 8, fontFamily: "inherit",
                background: (() => { const d = preview.diff; return d.characters.length + d.relations.length + d.world_settings.length === 0; })() ? T.panel3 : T.danger,
                color: (() => { const d = preview.diff; return d.characters.length + d.relations.length + d.world_settings.length === 0; })() ? T.text3 : "#fff",
                fontWeight: 650,
                cursor: (() => { const d = preview.diff; return d.characters.length + d.relations.length + d.world_settings.length === 0; })() ? "not-allowed" : "pointer",
              }}>
              确认回滚（{(() => { const d = preview.diff; return d.characters.length + d.relations.length + d.world_settings.length; })()} 处变更）
            </button>
          </div>
        </div>
      </div>
  );
}
