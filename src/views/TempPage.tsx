// 「🗂 临时文件」页：AI 出图的中间产物（入库失败 / 中途取消时会残留）。
//
// 拆分说明（2026-10-17）：从 App.tsx 抽出。本组件**不持有状态**，
// 数据与动作都由 App 传入 —— 这样拆分不会改变任何行为。

import { T } from "../theme";

export type TempFile = { name: string; size: number; age_secs: number };

export function TempPage(props: {
  tempFiles: TempFile[];
  loadTemp: () => Promise<void>;
  cleanTemp: () => Promise<void>;
}) {
  const { tempFiles, loadTemp, cleanTemp } = props;
  return (
    <div style={{ maxWidth: 980, margin: "0 auto" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 6 }}>
        <div style={{ fontSize: 15, fontWeight: 650 }}>🗂 临时文件</div>
        <span style={{ fontSize: 11.5, color: T.text3 }}>
          AI 出图的中间产物 · 共 {tempFiles.length} 个 · {(tempFiles.reduce((s, x) => s + x.size, 0) / 1024 / 1024).toFixed(2)} MB
        </span>
        <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
          <button onClick={() => void loadTemp()} style={{ height: 30, padding: "0 12px", borderRadius: 7, border: `1px solid ${T.line}`, background: "transparent", color: T.text2, fontSize: 12, cursor: "pointer", fontFamily: "inherit" }}>刷新</button>
          <button onClick={() => void cleanTemp()} disabled={tempFiles.length === 0}
            style={{ height: 30, padding: "0 12px", borderRadius: 7, border: "none", background: tempFiles.length ? T.accent : T.line, color: tempFiles.length ? "#231505" : T.text3, fontSize: 12, fontWeight: 600, cursor: tempFiles.length ? "pointer" : "default", fontFamily: "inherit" }}>清理全部</button>
        </div>
      </div>
      <p style={{ fontSize: 11.5, color: T.text3, margin: "0 0 12px", lineHeight: 1.7 }}>
        这里只存**出图过程中的中间图片**：生成成功并入库后会自动清理。
        但"入库失败 / 中途取消 / 批量只成功一部分"时它们会留下来，占着磁盘 —— 可以在这里清掉。
        <br />
        为安全起见，**5 分钟内修改过的文件不会被清理**（可能正在生成）。
      </p>
      {tempFiles.length === 0 ? (
        <div style={{ border: `1px dashed ${T.line}`, borderRadius: 12, padding: "36px 20px", textAlign: "center", color: T.text3, fontSize: 12 }}>
          没有临时文件，很干净 ✅
        </div>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
          {tempFiles.map((x) => (
            <div key={x.name} style={{ display: "flex", alignItems: "center", gap: 10, padding: "7px 12px", border: `1px solid ${T.line}`, borderRadius: 8, background: T.panel2, fontSize: 12 }}>
              <span style={{ color: T.text2, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", maxWidth: "58%", fontFamily: "monospace" }}>{x.name}</span>
              <span style={{ marginLeft: "auto", color: T.text3 }}>{(x.size / 1024).toFixed(0)} KB</span>
              <span style={{ color: x.age_secs < 300 ? T.warn : T.text3, minWidth: 92, textAlign: "right" }}>
                {x.age_secs < 300 ? `${x.age_secs} 秒前 · 受保护` : `${Math.floor(x.age_secs / 60)} 分钟前`}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
