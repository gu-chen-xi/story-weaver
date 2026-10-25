// 「🗄 数据管理」：存储概览 + 安全清理（用户 2026-10-18 定，方案 B）。
//
// 定位：设置 → 数据管理 打开的页面。
// 原则：
//   * 只清理**删了不影响剧情**的东西（调试记录 / 日志 / 旧备份 / 孤儿目录 / 出图临时文件）；
//   * 每项都写明"这是什么、删了会怎样"，并做二次确认；
//   * 故事库、立绘图片**不给清理按钮**（要删请去对应页面），只显示占用。
//
// 明确不做：源码目录的 `target/`（编译缓存）、cargo/WiX/NSIS 缓存 —— 用户的安装版里没有这些。

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { T } from "../theme";
import { useConfirm } from "../confirm";

export type TempFile = { name: string; size: number; age_secs: number };

export type DataItem = {
  key: string;
  label: string;
  bytes: number;
  count: number;
  hint: string;
  cleanable: boolean;
};

export type DataOverview = {
  data_dir: string;
  total_bytes: number;
  items: DataItem[];
};

type CleanResult = { removed: number; freed: number };

const mb = (n: number) => (n / 1024 / 1024).toFixed(2) + " MB";
const kb = (n: number) => (n / 1024).toFixed(1) + " KB";
const sizeOf = (n: number) => (n >= 1024 * 1024 ? mb(n) : kb(n));

export function TempPage(props: {
  tempFiles: TempFile[];
  loadTemp: () => Promise<void>;
  cleanTemp: () => Promise<void>;
}) {
  const confirm = useConfirm();
  const { tempFiles, loadTemp, cleanTemp } = props;
  const [ov, setOv] = useState<DataOverview | null>(null);
  const [busy, setBusy] = useState("");
  const [msg, setMsg] = useState("");
  const [debugMax, setDebugMax] = useState(300);

  const loadOverview = async () => {
    try {
      setOv(await invoke<DataOverview>("data_overview"));
    } catch (e) {
      setMsg("读取存储概览失败：" + String(e));
    }
  };

  useEffect(() => {
    void loadOverview();
    void invoke<number>("data_debug_max").then((n) => setDebugMax(n)).catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /// 统一执行一个清理动作：确认 → 调用 → 回显 → 刷新概览
  const runClean = async (
    key: string,
    label: string,
    question: string,
    fn: () => Promise<CleanResult>,
  ) => {
    if (!(await confirm({ title: `清理：${label}`, body: question, danger: true }))) return;
    setBusy(key);
    setMsg("");
    try {
      const r = await fn();
      setMsg(
        r.removed > 0
          ? `✅ ${label}：处理 ${r.removed} 项，释放 ${sizeOf(r.freed)}`
          : `${label}：没有可清理的内容`,
      );
      await loadOverview();
      await loadTemp();
    } catch (e) {
      setMsg(`❌ ${label}失败：` + String(e));
    } finally {
      setBusy("");
    }
  };

  /// 保存"自动保留条数"（失焦/回车时）
  const saveDebugMax = async (n: number) => {
    const v = Math.max(10, Math.min(100000, Math.floor(n) || 300));
    setDebugMax(v);
    try {
      const saved = await invoke<number>("data_set_debug_max", { rows: v });
      setDebugMax(saved);
      setMsg(`✅ 调试记录自动保留最近 ${saved} 条（每次生成前自动清理更旧的）`);
    } catch (e) {
      setMsg("❌ 保存失败：" + String(e));
    }
  };

  const btnStyle = (enabled: boolean) =>
    ({
      padding: "5px 11px",
      borderRadius: 7,
      border: `1px solid ${T.line}`,
      background: enabled ? T.panel2 : "transparent",
      color: enabled ? T.text1 : T.text3,
      fontSize: 11.5,
      cursor: enabled ? "pointer" : "default",
      fontFamily: "inherit",
    }) as const;

  const itemOf = (key: string) => ov?.items.find((x) => x.key === key);

  return (
    <div style={{ maxWidth: 980, margin: "0 auto" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 4 }}>
        <div style={{ fontSize: 15, fontWeight: 650 }}>🗄 数据管理</div>
        <span style={{ fontSize: 11.5, color: T.text3 }}>
          {ov ? `共占用 ${mb(ov.total_bytes)}` : "统计中…"}
        </span>
        <button onClick={() => void loadOverview()} disabled={!!busy}
          style={{ ...btnStyle(true), marginLeft: "auto" }}>刷新</button>
      </div>
      <p style={{ fontSize: 11.5, color: T.text3, margin: "0 0 14px", lineHeight: 1.7 }}>
        这里只清理<strong>删掉不影响剧情</strong>的东西（调试痕迹、日志、旧备份、临时文件）。
        故事库和立绘图片只显示占用，要删请到「故事列表」「立绘素材」里操作。
        {ov && (
          <>
            <br />
            数据目录：<code style={{ fontSize: 11 }}>{ov.data_dir}</code>
          </>
        )}
      </p>

      {msg && (
        <div style={{ fontSize: 12, marginBottom: 12, color: msg.startsWith("❌") ? T.danger : T.text1 }}>
          {msg}
        </div>
      )}

      {/* 存储概览 */}
      <div style={{ border: `1px solid ${T.line}`, borderRadius: 10, overflow: "hidden", marginBottom: 16 }}>
        {ov?.items.map((it) => (
          <div key={it.key} style={{ display: "flex", alignItems: "center", gap: 10,
            padding: "9px 12px", borderTop: `1px solid ${T.line}`, fontSize: 12 }}>
            <span style={{ width: 110, color: T.text1, fontWeight: it.cleanable ? 600 : 400 }}>{it.label}</span>
            <span style={{ width: 82, color: T.text2, fontFamily: "monospace" }}>{sizeOf(it.bytes)}</span>
            <span style={{ width: 92, color: T.text3, fontSize: 11 }}>
              {it.count >= 0 ? `${it.count} ${it.key === "stories" ? "个" : "项"}` : ""}
            </span>
            <span style={{ flex: 1, color: T.text3, fontSize: 11, lineHeight: 1.5 }}>{it.hint}</span>
            {it.cleanable ? (
              it.key === "debug" ? (
                <span style={{ display: "flex", gap: 6, alignItems: "center" }}>
                  <span title="每次生成前自动把更旧的调试记录删掉，防止无限增长"
                    style={{ fontSize: 10.5, color: T.text3, display: "inline-flex", alignItems: "center", gap: 3 }}>
                    自动保留
                    <input type="number" min={10} max={100000} value={debugMax}
                      onChange={(e) => setDebugMax(Number(e.target.value) || 300)}
                      onBlur={(e) => void saveDebugMax(Number(e.target.value))}
                      onKeyDown={(e) => { if (e.key === "Enter") void saveDebugMax(Number((e.target as HTMLInputElement).value)); }}
                      style={{ width: 52, padding: "2px 4px", borderRadius: 5, border: `1px solid ${T.line}`,
                        background: "#191c22", color: T.text1, fontSize: 10.5, fontFamily: "monospace", textAlign: "center" }} />
                    条
                  </span>
                  <button disabled={!!busy}
                    onClick={() => void runClean("debug50", "调试记录",
                      "只保留最近 50 条调试记录，其余删除？\n\n（只影响「生成参数」里回看历史，不影响剧情）",
                      () => invoke<CleanResult>("data_clean_debug", { keep: 50 }))}
                    style={btnStyle(!busy)}>保留最近 50</button>
                  <button disabled={!!busy}
                    onClick={() => void runClean("debug0", "调试记录",
                      "删除全部的调试记录？\n\n（只影响「生成参数」里回看历史，不影响剧情）",
                      () => invoke<CleanResult>("data_clean_debug", { keep: 0 }))}
                    style={btnStyle(!busy)}>全清</button>
                </span>
              ) : it.key === "logs" ? (
                <button disabled={!!busy}
                  onClick={() => void runClean("logs", "日志",
                    "清空所有日志文件？\n\n（文件保留，只是内容清空；不影响剧情）",
                    () => invoke<CleanResult>("data_clean_logs"))}
                  style={btnStyle(!busy)}>清空</button>
              ) : it.key === "backups" ? (
                <span style={{ display: "flex", gap: 6 }}>
                  <button disabled={!!busy}
                    onClick={() => void runClean("bakKeep", "旧备份",
                      "删除旧备份，只保留最新的一份当保险？",
                      () => invoke<CleanResult>("data_clean_backups", { keepLatest: true }))}
                    style={btnStyle(!busy)}>保留最新</button>
                  <button disabled={!!busy}
                    onClick={() => void runClean("bakAll", "旧备份",
                      "删除全部旧备份（包括最新那一份）？\n\n确认现在的故事都正常再删。",
                      () => invoke<CleanResult>("data_clean_backups", { keepLatest: false }))}
                    style={btnStyle(!busy)}>全删</button>
                </span>
              ) : it.key === "orphans" ? (
                <button disabled={!!busy}
                  onClick={() => void runClean("orphans", "孤儿项目目录",
                    "删除磁盘上不属于任何故事的 projects/<id> 目录？\n\n（这些目录在故事列表里已经看不到了）",
                    () => invoke<CleanResult>("data_clean_orphans"))}
                  style={btnStyle(!busy)}>清理</button>
              ) : it.key === "temp" ? (
                <button disabled={!!busy || tempFiles.length === 0}
                  onClick={() => void runClean("temp", "出图临时文件",
                    `清理全部 ${tempFiles.length} 个出图临时文件？`,
                    async () => {
                      const before = tempFiles.reduce((s, x) => s + x.size, 0);
                      await cleanTemp();
                      return { removed: tempFiles.length, freed: before };
                    })}
                  style={btnStyle(!busy && tempFiles.length > 0)}>
                  清理（{tempFiles.length}）
                </button>
              ) : null
            ) : (
              <span style={{ fontSize: 10.5, color: T.text3, opacity: 0.7 }}>不可清理</span>
            )}
          </div>
        ))}
        {!ov && <div style={{ padding: "14px 12px", fontSize: 12, color: T.text3 }}>正在统计…</div>}
      </div>

      {/* 出图临时文件明细（保留原「临时文件」页的信息） */}
      <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 6 }}>
        <div style={{ fontSize: 13, fontWeight: 600 }}>🗂 出图临时文件明细</div>
        <span style={{ fontSize: 11, color: T.text3 }}>
          {tempFiles.length} 个
          {itemOf("temp") ? ` · ${sizeOf(itemOf("temp")!.bytes)}` : ""}
        </span>
      </div>
      <p style={{ fontSize: 11.5, color: T.text3, margin: "0 0 10px", lineHeight: 1.7 }}>
        出图过程中的中间图片：生成成功并入库后会自动清理，这里剩下的多半是中途取消/失败留下的。
      </p>
      <div style={{ border: `1px solid ${T.line}`, borderRadius: 10, overflow: "hidden" }}>
        {tempFiles.length === 0 ? (
          <div style={{ padding: "14px 12px", fontSize: 12, color: T.text3 }}>干净的，没有临时文件。</div>
        ) : (
          tempFiles.map((f) => (
            <div key={f.name} style={{ display: "flex", gap: 10, padding: "7px 12px",
              borderTop: `1px solid ${T.line}`, fontSize: 11.5 }}>
              <span style={{ flex: 1, color: T.text2, overflow: "hidden", textOverflow: "ellipsis" }}>{f.name}</span>
              <span style={{ color: T.text3, fontFamily: "monospace" }}>{sizeOf(f.size)}</span>
              <span style={{ color: T.text3 }}>{Math.round(f.age_secs / 60)} 分钟前</span>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
