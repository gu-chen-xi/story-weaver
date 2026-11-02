// 「🧾 摘要管理」——导演监视器里的一个小面板：看/删/压阶段摘要。
//
// 为什么单独做一个组件：这里的交互（列表、删除、压缩、回显）都是自包含的，
// 塞进 App.tsx 只会让它重新长回去（App 已从 6788 行降到 2427 行，别再涨）。

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { T } from "../theme";
import { useConfirm } from "../confirm";

export type PhaseRow = {
  id: number;
  day_from: number;
  day_to: number;
  source_days: number;
  chars: number;
  created_at: string;
};

/// 「已压缩 / 未压缩 / 还能压几段」统计（后端 phase_plan_stats）
export type PhaseStats = {
  per_segment: number;
  summarized_days: number;
  covered_days: number;
  uncovered_days: number;
  remaining_batches: number;
  next_from: number | null;
  next_to: number | null;
};

export type PhaseManagerProps = {
  storyId: string | null;
  /// 有日摘要的天数 / 天数节点总数（由 App 传入，避免这里再查一遍）
  summarizedDays: number;
  totalDays: number;
  /// 摘要被删/被压后，让 App 重新装配（比如刷新时间线标记）
  onChanged?: () => void;
};

export function PhaseManager(props: PhaseManagerProps) {
  const confirm = useConfirm();
  const { storyId, summarizedDays, totalDays, onChanged } = props;
  const [rows, setRows] = useState<PhaseRow[]>([]);
  const [busy, setBusy] = useState(false);
  const [x, setX] = useState(10);
  const [info, setInfo] = useState("");
  const [buildTime, setBuildTime] = useState("");
  const [stats, setStats] = useState<PhaseStats | null>(null);

  const load = async () => {
    if (!storyId) { setRows([]); return; }
    try {
      setRows(await invoke<PhaseRow[]>("list_phase_rows", { storyId }));
    } catch (e) {
      setInfo("读取失败：" + String(e));
    }
  };

  const loadStats = async () => {
    if (!storyId) { setStats(null); return; }
    try {
      setStats(await invoke<PhaseStats>("phase_plan_stats", { storyId, minDays: x }));
    } catch { /* 统计失败不影响主列表 */ }
  };

  useEffect(() => { void load(); /* eslint-disable-next-line react-hooks/exhaustive-deps */ }, [storyId]);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => { void loadStats(); }, [storyId, x]);
  useEffect(() => {
    void invoke<number>("get_phase_min_days").then((n) => setX(n)).catch(() => {});
    // 构建时间：用来确认"现在跑的是不是刚构建的那一版"（排查过一次 stale build）
    void invoke<{ version: string; build_time: string }>("get_build_info")
      .then((b) => setBuildTime(`${b.version} · ${b.build_time}`))
      .catch(() => {});
  }, []);

  const compress = async () => {
    if (!storyId) return;
    setBusy(true);
    setInfo("");
    try {
      const made = await invoke<PhaseRow | null>("run_phase_summary", {
        storyId, minDays: x, saveAsDefault: true,
      });
      if (!made) {
        setInfo(`未压缩不足 ${x} 天（现在 ${stats?.uncovered_days ?? "?"} 天），先攒着`);
      } else {
        setInfo(`✅ 已压第 ${(made as unknown as { day_from: number }).day_from ?? "-"} 段`);
        setInfo(`✅ 已压「第 ${(made as unknown as { day_from: number; day_to: number }).day_from}~${(made as unknown as { day_to: number }).day_to} 天」`);
      }
      await load();
      await loadStats();
      onChanged?.();
    } catch (e) {
      setInfo("❌ 压缩失败：" + String(e));
    } finally {
      setBusy(false);
    }
  };

  const del = async (id: number, label: string) => {
    if (!storyId) return;
    if (!(await confirm({ title: "删除阶段摘要", body: `删除「${label}」这段阶段摘要？\n\n只删这段汇总，日摘要不受影响。`, danger: true }))) return;
    try {
      await invoke<boolean>("delete_phase_row", { storyId, id });
      await load();
      onChanged?.();
      setInfo("🗑️ 已删除 1 条阶段摘要");
    } catch (e) {
      setInfo("❌ 删除失败：" + String(e));
    }
  };


  const dedupe = async () => {
    if (!storyId) return;
    setBusy(true);
    setInfo("");
    try {
      const n = await invoke<number>("dedupe_phase_rows", { storyId });
      await load();
      await loadStats();
      onChanged?.();
      setInfo(n > 0 ? `🧹 已清理 ${n} 条重叠（每组保留最新的一条）` : "没有重叠，无需清理");
    } catch (e) {
      setInfo("❌ 清理失败：" + String(e));
    } finally {
      setBusy(false);
    }
  };

  const covered = new Set<number>();
  rows.forEach((r) => { for (let d = r.day_from; d <= r.day_to; d++) covered.add(d); });

  return (
    <div>
      <div style={{ fontSize: 10.5, color: T.text3, letterSpacing: 1.1, marginBottom: 8,
        display: "flex", alignItems: "center", gap: 6 }}>
        <span>🧾 摘要管理</span>
        <span style={{ marginLeft: "auto" }}>{summarizedDays}/{totalDays} 天有摘要</span>
      </div>
      {buildTime && (
        <div title="构建时间：确认你跑的是不是最新那一版"
          style={{ fontSize: 9.5, color: T.text3, opacity: 0.7, marginBottom: 6 }}>
          构建 {buildTime}
        </div>
      )}

      {/* 两个容器：已压缩 / 未压缩（2026-11-02 用户要求） */}
      <div style={{ display: "flex", gap: 6, marginBottom: 6 }}>
        <div style={{ flex: 1, padding: "6px 8px", borderRadius: 7, border: `1px solid ${T.line}`, background: T.panel2 }}>
          <div style={{ fontSize: 10, color: T.text3 }}>已压缩</div>
          <div style={{ fontSize: 13, color: T.accent, fontWeight: 600 }}>{stats ? stats.covered_days : covered.size} 天</div>
          <div style={{ fontSize: 10, color: T.text3 }}>{rows.length} 段</div>
        </div>
        <div style={{ flex: 1, padding: "6px 8px", borderRadius: 7, border: `1px solid ${T.line}`, background: T.panel2 }}>
          <div style={{ fontSize: 10, color: T.text3 }}>未压缩</div>
          <div style={{ fontSize: 13, color: T.text1, fontWeight: 600 }}>{stats ? stats.uncovered_days : "—"} 天</div>
          <div style={{ fontSize: 10, color: T.text3 }}>
            {stats
              ? (stats.remaining_batches > 0
                  ? `还能压 ${stats.remaining_batches} 段`
                  : `不足 ${stats.per_segment} 天，先攒着`)
              : ""}
          </div>
        </div>
      </div>
      <div style={{ fontSize: 10.5, color: T.text3, marginBottom: 8 }}>
        {rows.length === 0 && (stats?.uncovered_days ?? 0) === 0
          ? "还没有日摘要（攒够天数后会自动压，也可手动压）"
          : stats && stats.next_from != null
            ? `下一段：第 ${stats.next_from}~${stats.next_to} 天（每段 ${stats.per_segment} 天）`
            : `未满一段（每段 ${stats?.per_segment ?? x} 天），先攒着；已压过的天不会再参与`}
      </div>

      {/* 手动压缩 */}
      <div style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 8 }}>
        <span style={{ fontSize: 11, color: T.text3 }}>每段</span>
        <input type="number" min={2} max={200} value={x}
          onChange={(e) => setX(Math.max(2, Math.min(200, Number(e.target.value) || 10)))}
          style={{ width: 46, padding: "3px 5px", borderRadius: 6, border: `1px solid ${T.line}`,
            background: "#191c22", color: T.text1, fontSize: 11, fontFamily: "monospace", textAlign: "center" }} />
        <span style={{ fontSize: 11, color: T.text3 }}>天</span>
        <button onClick={() => void compress()} disabled={busy || !storyId}
          title="按这个天数重新压一段（会清掉区间重叠的旧段）；这个值会被记住"
          style={{ marginLeft: "auto", flex: 1, fontSize: 11, padding: "4px 8px", borderRadius: 6,
            border: `1px solid ${T.line}`, background: busy ? T.panel3 : "#191c22",
            color: busy ? T.text3 : T.text1, cursor: busy ? "wait" : "pointer", fontFamily: "inherit" }}>
          {busy ? "压缩中…" : "压一段"}
        </button>
      </div>

      {info && (
        <div style={{ fontSize: 11, marginBottom: 8, lineHeight: 1.6,
          color: info.startsWith("❌") ? T.danger : T.text2 }}>{info}</div>
      )}

      {/* 列表 */}
      <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
        {rows.map((r) => (
          <div key={r.id} style={{ display: "flex", alignItems: "center", gap: 6, padding: "5px 8px",
            borderRadius: 7, border: `1px solid ${T.line}`, background: T.panel2, fontSize: 11 }}>
            <span style={{ color: T.accent, fontWeight: 600 }}>第 {r.day_from}~{r.day_to} 天</span>
            <span style={{ color: T.text3 }}>{r.source_days} 天素材 · {r.chars} 字</span>
            <span onClick={() => void del(r.id, `第 ${r.day_from}~${r.day_to} 天`)}
              title="删除这段阶段摘要（日摘要不受影响）"
              style={{ marginLeft: "auto", cursor: "pointer", color: T.text3 }}>✕</span>
          </div>
        ))}
      </div>

      {rows.length > 1 && (
        <button onClick={() => void dedupe()} disabled={busy}
          title="同一批天被多条阶段摘要覆盖时，只保留最新的一条（其它删掉）——否则【前情提要】用哪条取决于排序"
          style={{ marginTop: 6, width: "100%", fontSize: 11, padding: "4px 8px", borderRadius: 6,
            border: `1px solid ${T.line}`, background: "#191c22", color: T.text2,
            cursor: busy ? "wait" : "pointer", fontFamily: "inherit" }}>
          🧹 清理重叠（{rows.length} 段 → 保留最新）
        </button>
      )}

      {rows.length > 0 && (
        <div style={{ fontSize: 10.5, color: T.text3, marginTop: 6, lineHeight: 1.6 }}>
          【前情提要】按"最小区间"选段：多条并存时旧的宽区间不会盖住新压的。
        </div>
      )}
    </div>
  );
}
