// 「⚖ 合规与责任」页。
//
// 拆分说明（2026-10-17）：从 App.tsx 抽出。本组件**不持有状态** ——
// `complianceAck` / `checklistDone` 仍由 App 持有并传入，行为完全不变。

import type { Dispatch, SetStateAction } from "react";
import { T } from "../theme";
import { renderBold } from "../lib/rich";
import { COMPLIANCE_CASES, COMPLIANCE_CHECKLIST, SOFTWARE_SCOPE, COMPLIANCE_LAWS, LICENSE_NOTICE, NOTICE_VERSION, USER_DUTIES, REQUIRED_NOTICES } from "../compliance";

export function CompliancePage(props: {
  complianceAck: boolean;
  ackCompliance: () => void;
  checklistDone: Set<string>;
  setChecklistDone: Dispatch<SetStateAction<Set<string>>>;
}) {
  const { complianceAck, ackCompliance, checklistDone, setChecklistDone } = props;
  return (
        <div style={{ maxWidth: 920 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 6, flexWrap: "wrap" }}>
            <h3 style={{ margin: 0 }}>⚖ 合规与责任</h3>
            {complianceAck
              ? <span style={{ fontSize: 10.5, padding: "2px 9px", borderRadius: 9, border: `1px solid ${T.ok}`, color: T.ok }}>已同意当前条款 · {NOTICE_VERSION}</span>
              : <span style={{ fontSize: 10.5, padding: "2px 9px", borderRadius: 9, border: `1px solid ${T.warn}`, color: T.warn }}>尚未同意当前条款</span>}
            <span style={{ fontSize: 10.5, color: T.text3 }}>条款版本 {NOTICE_VERSION}</span>
            {!complianceAck && <button onClick={ackCompliance} style={{ marginLeft: "auto", fontSize: 12, padding: "4px 12px" }}>我已阅读并同意当前版本</button>}
          </div>
          {/* 许可协议与免责声明：放最上方，防止有人拿本项目源码去做商业 SaaS 事后牵连作者 */}
          <div style={{ border: `1px solid ${T.accent}`, borderRadius: 10, background: T.accentSoft, padding: "12px 14px", marginBottom: 14 }}>
            <div style={{ fontSize: 13, fontWeight: 650, marginBottom: 7, color: T.accent }}>
              📜 许可协议与免责声明
            </div>
            <div style={{ fontSize: 11.5, color: T.text2, lineHeight: 1.85 }}>
              {LICENSE_NOTICE.lines.map((x, i) => (
                <div key={i} style={{ marginBottom: 2 }}>· {renderBold(x)}</div>
              ))}
              <div style={{ marginTop: 8, color: T.text3 }}>{LICENSE_NOTICE.disclaimer}</div>
              <div style={{ marginTop: 8 }}>
                <span style={{ color: T.text3 }}>协议：</span>
                <a href={LICENSE_NOTICE.licenseUrl} target="_blank" rel="noreferrer"
                  style={{ color: T.accent, textDecoration: "underline" }}>
                  {LICENSE_NOTICE.license}（{LICENSE_NOTICE.licenseFull}）
                </a>
                <span style={{ color: T.text3 }}> · 完整条款见仓库 LICENSE 及附录</span>
              </div>
            </div>
          </div>

          <p style={{ fontSize: 12, color: T.text3, lineHeight: 1.8, marginBottom: 14 }}>
            这一页说明：<b style={{ color: T.text2 }}>AI 生成内容的法律义务在谁身上</b>、本软件做什么与不做什么、以及你发布或商用前该过一遍哪些点。
            内容为工程整理，<b style={{ color: T.warn }}>不构成法律意见</b>；具体问题请咨询专业律师。
          </p>

          <div style={{ border: `1px solid ${T.line}`, borderRadius: 10, background: T.panel, padding: "12px 14px", marginBottom: 14 }}>
            <div style={{ fontSize: 13, fontWeight: 650, marginBottom: 8 }}>一、责任边界：本软件是什么、不是什么</div>
            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 14 }}>
              <div>
                <div style={{ fontSize: 11.5, color: T.ok, fontWeight: 600, marginBottom: 5 }}>✔ 本软件会做的</div>
                {SOFTWARE_SCOPE.can.map((x, i) => (
                  <div key={i} style={{ fontSize: 11.5, color: T.text2, lineHeight: 1.8 }}>· {x}</div>
                ))}
              </div>
              <div>
                <div style={{ fontSize: 11.5, color: T.warn, fontWeight: 600, marginBottom: 5 }}>✘ 本软件不会做的</div>
                {SOFTWARE_SCOPE.cannot.map((x, i) => (
                  <div key={i} style={{ fontSize: 11.5, color: T.text2, lineHeight: 1.8 }}>· {x}</div>
                ))}
              </div>
            </div>
            <div style={{ fontSize: 11.5, color: T.text3, marginTop: 10, lineHeight: 1.8, borderTop: `1px solid ${T.lineSoft}`, paddingTop: 8 }}>
              一句话：<b style={{ color: T.text1 }}>生成服务由你选择并承担其条款，内容合规义务由你（使用者）承担；本软件只做本地搬运与记录，并且不帮你抹掉任何 AI 标识。</b>
            </div>
          </div>

          {/* 一之二：使用者义务（软件不做审查，这些必须由使用者守住） */}
          <div style={{ border: `1px solid ${T.line}`, borderRadius: 10, background: T.panel, padding: "12px 14px", marginBottom: 14 }}>
            <div style={{ fontSize: 13, fontWeight: 650, marginBottom: 8 }}>二、使用者义务（你这边要守住的）</div>
            {USER_DUTIES.map((x, i) => (
              <div key={i} style={{ fontSize: 11.5, color: T.text2, lineHeight: 1.85 }}>· {renderBold(x)}</div>
            ))}
          </div>

          {/* 一之三：用户指定的两句声明（与 README / 用户协议用完全一致的措辞） */}
          <div style={{ border: `1px solid ${T.danger}`, borderRadius: 10, background: "rgba(240,85,77,.08)", padding: "12px 14px", marginBottom: 14 }}>
            <div style={{ fontSize: 13, fontWeight: 650, marginBottom: 8, color: T.danger }}>三、必须明确的声明</div>
            {REQUIRED_NOTICES.map((x, i) => (
              <div key={i} style={{ fontSize: 12.5, color: T.text1, lineHeight: 1.9, fontWeight: 600 }}>
                {i + 1}. {x}
              </div>
            ))}
            <div style={{ fontSize: 11, color: T.text3, marginTop: 8, lineHeight: 1.75 }}>
              这两条同时写入仓库 README 与 LICENSE（用户协议），措辞保持一致。
            </div>
          </div>

          <div style={{ fontSize: 13, fontWeight: 650, marginBottom: 8 }}>四、按场景看该怎么做</div>
          <div style={{ display: "flex", flexDirection: "column", gap: 10, marginBottom: 14 }}>
            {COMPLIANCE_CASES.map((c) => {
              const rc = c.risk === "高" ? T.danger : c.risk === "中" ? T.warn : T.ok;
              return (
                <div key={c.id} style={{ border: `1px solid ${T.line}`, borderRadius: 10, background: T.panel, padding: "11px 13px" }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 7, flexWrap: "wrap" }}>
                    <span style={{ fontSize: 16 }}>{c.icon}</span>
                    <b style={{ fontSize: 12.5 }}>{c.title}</b>
                    <span style={{ fontSize: 10, padding: "1px 8px", borderRadius: 9, border: `1px solid ${rc}`, color: rc }}>风险 {c.risk}</span>
                  </div>
                  {c.dos.map((x, i) => (
                    <div key={`d${i}`} style={{ fontSize: 11.5, color: T.text2, lineHeight: 1.85 }}>
                      <span style={{ color: T.ok }}>✔ </span>{renderBold(x)}
                    </div>
                  ))}
                  {c.donts.map((x, i) => (
                    <div key={`n${i}`} style={{ fontSize: 11.5, color: T.text2, lineHeight: 1.85 }}>
                      <span style={{ color: T.danger }}>✘ </span>{renderBold(x)}
                    </div>
                  ))}
                </div>
              );
            })}
          </div>

          <div style={{ border: `1px solid ${T.line}`, borderRadius: 10, background: T.panel, padding: "12px 14px", marginBottom: 14 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 8, flexWrap: "wrap" }}>
              <span style={{ fontSize: 13, fontWeight: 650 }}>五、发布 / 商用前自查</span>
              <span style={{ fontSize: 11, color: T.text3 }}>已勾 {checklistDone.size} / {COMPLIANCE_CHECKLIST.length}</span>
              {checklistDone.size > 0 && (
                <button onClick={() => setChecklistDone(new Set())} style={{ fontSize: 11, padding: "2px 9px" }}>清空</button>
              )}
            </div>
            {COMPLIANCE_CHECKLIST.map((it) => {
              const on = checklistDone.has(it.id);
              return (
                <label key={it.id} style={{ display: "flex", alignItems: "flex-start", gap: 8, fontSize: 11.5, color: on ? T.text3 : T.text2, lineHeight: 1.9, cursor: "pointer", textDecoration: on ? "line-through" : "none" }}>
                  <input type="checkbox" checked={on} style={{ marginTop: 5, accentColor: T.accent }}
                    onChange={(e) => setChecklistDone((prev) => { const n = new Set(prev); if (e.target.checked) n.add(it.id); else n.delete(it.id); return n; })} />
                  {it.text}
                </label>
              );
            })}
            <div style={{ fontSize: 10.5, color: T.text3, marginTop: 8 }}>勾选只在本次会话内保留，方便对照检查，不会被上传或外发。</div>
          </div>

          <div style={{ fontSize: 13, fontWeight: 650, marginBottom: 8 }}>六、相关法规（去看原文）</div>
          <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            {COMPLIANCE_LAWS.map((l) => (
              <div key={l.name} style={{ border: `1px solid ${T.line}`, borderRadius: 10, background: T.panel, padding: "10px 13px" }}>
                <div style={{ fontSize: 12.5, fontWeight: 600, marginBottom: 3 }}>{l.name}</div>
                <div style={{ fontSize: 11.5, color: T.text3, lineHeight: 1.7 }}>{l.note}</div>
              </div>
            ))}
          </div>

          <div style={{ fontSize: 10.5, color: T.text3, marginTop: 14, lineHeight: 1.8 }}>
            用法提示：立绘素材页可按「来源 → AI 生成」筛出所有 AI 图；导出的文件名带 <b style={{ color: "#ffd27d" }}>_AI</b> 后缀，
            并附「导出清单.json」（含提示词 / 负向 / 种子，兼作创作过程证据）。
          </div>
        </div>
  );
}
