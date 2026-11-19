// 首页 / 启动台（2026-11-19）。
//
// ## 它解决什么
//
// 原来 `view === "home"` **没有渲染分支** —— 打开软件只有一句「从左侧导航选择功能」的空白页。
// 这里按用户给的参考页（`code.html`）做成居中启动台：问候 + 「继续创作」+ 最近编辑。
//
// ## 数据从哪来
//
// **只用 `stories`（故事列表）这一份数据**（用户 2026-11-19 定："以故事列表为底座，
// 到时候要读有几个数据读故事列表里面有几个就行，不用去代码里面查找"）。
// 所以这里没有节点数/消息数 —— 那些要去开每个故事的 story.db 数，代价不对等。
// `stories` 后端已按 `updated_at DESC` 排序，`stories[0]` 就是"上次写的那个"。
//
// ## 为什么只显示 3 条
//
// 首页只负责"接着干"，管理动作（删除 / 改前史 / 刷新 / 新建表单）留在「故事列表」页；
// 「全部故事 →」跳过去。两页不重复维护。
//
// 动效（参考页同款）：整页淡入 `.dw-page-in`、按钮/条目悬停上浮 `.dw-lift` / `.dw-recent`；
// 类定义在 `App.tsx` 的全局 `<style>`（项目约定：全局样式只写一处）。

import { T } from "../theme";
import { greetingFor, relativeTime } from "../lib/time";
import type { Story } from "../types";

export type HomePageProps = {
  stories: Story[];
  /// 进某个故事（= 继续创作 / 点最近编辑）
  onContinue: (s: Story) => void;
  /// 新建故事：跳故事页并展开表单
  onNew: () => void;
  /// 全部故事：跳故事页
  onAll: () => void;
};

const ghostBtn: React.CSSProperties = {
  height: 42,
  padding: "0 20px",
  borderRadius: 10,
  border: `1px solid ${T.line}`,
  background: T.panel2,
  color: T.text2,
  fontSize: 14,
  cursor: "pointer",
  fontFamily: "inherit",
};

export function HomePage(props: HomePageProps) {
  const { stories, onContinue, onNew, onAll } = props;
  const last = stories[0] ?? null;
  const recent = stories.slice(0, 3);
  const total = stories.length;

  return (
    <div className="dw-page-in" style={{ maxWidth: 560, margin: "0 auto", padding: "72px 24px 40px" }}>
      <div style={{ fontSize: 26, fontWeight: 700, color: T.text1, letterSpacing: 0.2 }}>{greetingFor()}</div>
      <div style={{ fontSize: 13.5, color: T.text3, lineHeight: 1.75, marginTop: 8, marginBottom: 28 }}>
        {last
          ? `从《${last.title}》停下的地方继续，或者开一个新故事。`
          : "还没有故事，先开一个吧 —— 建好之后这里会记住你上次写到哪。"}
      </div>

      <div style={{ display: "flex", gap: 12, marginBottom: 40, flexWrap: "wrap" }}>
        {last && (
          <button
            className="dw-lift"
            onClick={() => onContinue(last)}
            style={{
              height: 42, padding: "0 22px", borderRadius: 10, border: "none",
              background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505",
              fontSize: 14, fontWeight: 700, cursor: "pointer", fontFamily: "inherit",
              boxShadow: "0 3px 14px rgba(255,122,44,.28)",
            }}
          >
            继续创作 · {last.title}
          </button>
        )}
        <button className="dw-lift" onClick={onNew} style={ghostBtn}>＋ 新建故事</button>
      </div>

      <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 12 }}>
        <span style={{ fontSize: 12, color: T.text3, letterSpacing: 1 }}>最近编辑</span>
        <span style={{ fontSize: 11, color: T.text3 }}>共 {total} 个故事</span>
        <div style={{ flex: 1 }} />
        {total > 0 && (
          <button
            onClick={onAll}
            style={{ background: "none", border: "none", color: T.accent, cursor: "pointer", fontSize: 12, padding: 0, fontFamily: "inherit" }}
          >
            全部故事 →
          </button>
        )}
      </div>

      {total === 0 ? (
        <div style={{ border: `1px dashed ${T.line}`, borderRadius: 12, padding: "32px 20px", textAlign: "center", color: T.text3, background: "transparent" }}>
          <div style={{ fontSize: 26, marginBottom: 10 }}>📚</div>
          <div style={{ fontSize: 13 }}>还没有故事</div>
          <div style={{ fontSize: 11.5, color: T.text3, marginTop: 6, lineHeight: 1.7 }}>
            建一个之后，这里会出现「继续创作」和最近编辑。
          </div>
        </div>
      ) : (
        recent.map((s) => {
          const rel = relativeTime(s.updated_at);
          return (
            <button
              key={s.id}
              className="dw-recent"
              onClick={() => onContinue(s)}
              style={{
                display: "flex", alignItems: "center", gap: 14, width: "100%", textAlign: "left",
                background: T.panel, border: `1px solid ${T.line}`, borderRadius: 12,
                padding: "14px 16px", marginBottom: 10, cursor: "pointer",
                color: T.text1, fontFamily: "inherit",
              }}
            >
              <span style={{
                width: 40, height: 40, flex: "0 0 40px", borderRadius: 10,
                background: T.panel2, display: "flex", alignItems: "center", justifyContent: "center",
                fontSize: 17, fontWeight: 700, color: T.accent,
              }}>
                {s.title.trim().slice(0, 1) || "📖"}
              </span>
              <span style={{ flex: 1, minWidth: 0 }}>
                <b style={{ display: "block", fontSize: 14.5, color: T.text1, marginBottom: 3, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                  {s.title}
                </b>
                <span style={{ fontSize: 12, color: T.text3 }}>
                  起点 {s.time_origin}{rel ? ` · ${rel}` : ""}{s.prologue ? " · 📜 有前史" : ""}
                </span>
              </span>
              <span className="dw-go" style={{ color: T.accent, fontSize: 13, flexShrink: 0 }}>进入 →</span>
            </button>
          );
        })
      )}
    </div>
  );
}
