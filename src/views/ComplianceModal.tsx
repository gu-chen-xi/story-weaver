// 首次启动的「使用前必读」强制确认弹窗（2026-11-04 从 App.tsx 抽出，第二刀）。
//
// 两条硬约束（都是踩过的坑）：
//   ① 必须在**组件根层**渲染 —— 放进 overflow: hidden 的容器里时 position: fixed 会被裁，
//      只显示成页面一小块；
//   ② 只留一个出口（勾选后才能进）—— 曾经有「去读完整说明」按钮会关掉弹窗，等于开后门。

import { LICENSE_NOTICE, NOTICE_VERSION, REQUIRED_NOTICES } from "../compliance";
import { renderBold } from "../lib/rich";
import { T } from "../theme";

export type ComplianceModalProps = {
  /** 是否显示（首次启动 / 条款版本变更时为 true） */
  open: boolean;
  /** 勾选状态：不勾不能进 */
  agreed: boolean;
  onAgree: (v: boolean) => void;
  /** 勾选后点「同意并开始使用」 */
  onConfirm: () => void;
};

export function ComplianceModal({ open, agreed, onAgree, onConfirm }: ComplianceModalProps) {
  if (!open) return null;
  return (
        <div style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,.72)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 1000, padding: 20 }}>
          <div style={{ width: 640, maxWidth: "100%", maxHeight: "86vh", overflowY: "auto", background: T.win, border: `1px solid ${T.line}`, borderRadius: 12, padding: "18px 20px", boxShadow: "0 30px 80px rgba(0,0,0,.6)" }}>
            <div style={{ fontSize: 16, fontWeight: 650, marginBottom: 4 }}>⚖ 使用前必读</div>
            <div style={{ fontSize: 11, color: T.text3, marginBottom: 12 }}>
              首次使用请确认以下内容。本弹窗需勾选后才能继续（确认一次即可，之后不再出现）。
            </div>
            {/* 协议与免责摘要 */}
            <div style={{ border: `1px solid ${T.accent}`, borderRadius: 8, background: T.accentSoft, padding: "10px 12px", marginBottom: 12 }}>
              <div style={{ fontSize: 12, fontWeight: 650, color: T.accent, marginBottom: 5 }}>📜 许可协议与免责声明</div>
              {LICENSE_NOTICE.lines.map((x, i) => (
                <div key={i} style={{ fontSize: 11.5, color: T.text2, lineHeight: 1.75 }}>· {renderBold(x)}</div>
              ))}
            </div>
            <div style={{ fontSize: 12.5, color: T.text2, lineHeight: 1.9 }}>
              欢迎使用本软件。它只是运行在你本机的工具，有几件事先说清楚：
              <div style={{ marginTop: 10, paddingLeft: 4 }}>
                · 它<b>不提供</b>生成式 AI 服务，也<b>不代你申请</b>任何接口；调用的是<b>你自己配置</b>的模型与绘图服务
                <br />· 它<b>不上传</b>你的数据：故事、角色、素材都留在本机
                <br />· 它<b>不会</b>帮你抹掉 AI 标识；反过来，绘图功能默认会在图上写入「AI」角标与元数据标识
                <br />· AI 生成内容的<b>合规义务在使用者</b>：对外发布请显式标注「AI 生成」，不要删除或篡改标识
                <br />· 不要用真人姓名或照片生成其形象；不要生成违法内容
                <br />· 不要生成损害心理健康的内容（自伤、暴力、极端仇恨、成瘾诱导等）；不诱导 AI 绕过服务方安全过滤
              </div>
              {/* 用户指定必须包含的两句（措辞与 README / LICENSE 保持一致） */}
              <div style={{ marginTop: 10, padding: "9px 11px", borderRadius: 8, border: `1px solid ${T.danger}`, background: "rgba(240,85,77,.08)", fontSize: 12, color: T.text1, lineHeight: 1.85, fontWeight: 600 }}>
                {REQUIRED_NOTICES.map((x, i) => (<div key={i}>{i + 1}. {x}</div>))}
              </div>
              <div style={{ marginTop: 12, fontSize: 11.5, color: T.text3, lineHeight: 1.8 }}>
                依据：《人工智能生成合成内容标识办法》（2025-09-01 施行）、《生成式人工智能服务管理暂行办法》等。
                完整说明与场景指引见左侧导航「⚖ 合规与责任」；本提示为工程整理，不构成法律意见。
              </div>
            </div>
            {/* 强制勾选：不勾不能进 */}
            <label style={{ display: "flex", alignItems: "flex-start", gap: 8, marginTop: 14, cursor: "pointer",
              border: `1px solid ${agreed ? T.ok : T.line}`, borderRadius: 8, padding: "10px 12px", background: agreed ? "rgba(55,201,120,.08)" : "transparent" }}>
              <input type="checkbox" checked={agreed} onChange={(e) => onAgree(e.target.checked)}
                style={{ marginTop: 3, cursor: "pointer", accentColor: T.accent }} />
              <span style={{ fontSize: 12, color: T.text2, lineHeight: 1.75 }}>
                我已阅读并同意上述许可协议与免责声明；我理解 <b>AI 生成内容的标识与合规义务由我（使用者）承担</b>，
                不会删除或篡改 AI 标识，也不会用本软件生成违法内容或未经授权使用他人肖像。
              </span>
            </label>

            <div style={{ display: "flex", gap: 10, justifyContent: "space-between", alignItems: "center", marginTop: 14, flexWrap: "wrap" }}>
              {/* 刻意只留一个出口：没有"先去别处看看"这类按钮。
                  曾经有一个「去读完整说明」，它会关掉弹窗并跳页 —— 等于给强制确认开了后门（用户实测发现）。
                  现在不勾选就无法离开弹窗；完整说明在确认后可从左侧导航「合规与责任」随时查阅。 */}
              <span style={{ fontSize: 11, color: T.text3 }}>
                勾选后即可开始使用；完整说明在左侧导航「⚖ 合规与责任」，随时可查。
                <br />条款版本 <b style={{ color: T.text2 }}>{NOTICE_VERSION}</b>
                <span>（条款内容更新后需重新确认一次）</span>
              </span>
              <button disabled={!agreed}
                onClick={() => { if (agreed) onConfirm(); }}
                title={agreed ? "" : "请先勾选上方确认项"}
                style={{ fontSize: 12.5, padding: "7px 18px", border: "none", borderRadius: 8,
                  background: agreed ? "linear-gradient(135deg,#FF7A18,#ff9a45)" : T.panel3,
                  color: agreed ? "#231505" : T.text3,
                  fontWeight: 650, cursor: agreed ? "pointer" : "not-allowed", fontFamily: "inherit" }}>
                {agreed ? "同意并开始使用" : "请先勾选确认"}
              </button>
            </div>
          </div>
        </div>
  );
}
