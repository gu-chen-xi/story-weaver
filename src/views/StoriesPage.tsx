// 「StoriesPage」（从 App.tsx 拆分，2026-10-17）。
// 只搬视图：state 与处理函数仍在 App，通过 props 传入。

import type { Dispatch, SetStateAction } from "react";
import { T } from "../theme";
import type { Story } from "../types";

export type StoriesPageProps = {
  createStory: () => Promise<void>;
  enterStory: (s: Story) => Promise<void>;
  loadStories: () => Promise<void>;
  newOrigin: string;
  newTitle: string;
  removeStory: (id: number, title: string) => Promise<void>;
  setNewOrigin: Dispatch<SetStateAction<string>>;
  setNewTitle: Dispatch<SetStateAction<string>>;
  setShowNew: Dispatch<SetStateAction<boolean>>;
  showNew: boolean;
  stories: Story[];
};

export function StoriesPage(props: StoriesPageProps) {
  const { createStory, enterStory, loadStories, newOrigin, newTitle, removeStory, setNewOrigin, setNewTitle, setShowNew, showNew, stories } = props;
  return (
(
            <div style={{ width: "100%" }}>
              <div style={{ display: "flex", gap: 12, alignItems: "center", marginBottom: 18 }}>
                <button onClick={() => setShowNew(!showNew)} style={{ height: 34, padding: "0 16px", borderRadius: 8, border: showNew ? `1px solid ${T.line}` : "none", background: showNew ? "#191c22" : "linear-gradient(135deg,#FF7A18,#ff9a45)", color: showNew ? T.text2 : "#231505", fontWeight: showNew ? 400 : 650, cursor: "pointer", fontSize: 13 }}>{showNew ? "取消" : "+ 新建故事"}</button>
                <button onClick={loadStories} style={{ height: 34, padding: "0 14px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, cursor: "pointer", fontSize: 13 }}>刷新</button>
                <span style={{ fontSize: 12, color: T.text3 }}>{stories.length} 个故事</span>
              </div>

              {showNew && (
                <div style={{ border: `1px solid ${T.line}`, padding: 16, borderRadius: 12, marginBottom: 16, background: T.panel }}>
                  <h3 style={{ marginTop: 0, fontSize: 14 }}>故事开幕</h3>
                  <p style={{ fontSize: 12.5, color: T.text3, marginBottom: 12 }}>设定绝对时间起点。起点固定后，"X 天后"由系统自动计算。</p>
                  <div style={{ display: "grid", gridTemplateColumns: "120px 1fr", gap: 8, alignItems: "center" }}>
                    <label style={{ fontSize: 12, color: T.text2 }}>故事标题</label>
                    <input value={newTitle} onChange={(e) => setNewTitle(e.target.value)} placeholder="例如：盛夏的雨" style={{ padding: "7px 10px" }} />
                    <label style={{ fontSize: 12, color: T.text2 }}>时间起点</label>
                    <input value={newOrigin} onChange={(e) => setNewOrigin(e.target.value)} placeholder="2026-01-01 或 架空纪元" style={{ padding: "7px 10px" }} />
                  </div>
                  <div style={{ marginTop: 12 }}>
                    <button onClick={createStory} style={{ height: 32, padding: "0 18px", borderRadius: 8, border: "none", background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505", cursor: "pointer", fontSize: 13, fontWeight: 650 }}>创建</button>
                  </div>
                </div>
              )}

              <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                {stories.length === 0 && (
                  <div style={{ border: `1px dashed ${T.line}`, borderRadius: 12, padding: "56px 20px", textAlign: "center", color: T.text3, background: "transparent" }}>
                    <div style={{ fontSize: 26, marginBottom: 10 }}>📚</div>
                    <div style={{ fontSize: 13 }}>还没有故事，点上方"+ 新建故事"开始。</div>
                    <div style={{ fontSize: 11.5, color: T.text3, marginTop: 6 }}>创建后将进入导演工作台（时间线 / 角色 / 世界观 / 演进提案）</div>
                  </div>
                )}
                {stories.map((s) => (
                  <div
                    key={s.id}
                    onClick={() => enterStory(s)}
                    style={{
                      border: `1px solid ${T.line}`,
                      padding: "13px 16px",
                      borderRadius: 10,
                      display: "flex",
                      justifyContent: "space-between",
                      alignItems: "center",
                      cursor: "pointer",
                      background: T.panel,
                      transition: "border-color .15s, background .15s",
                    }}
                    onMouseEnter={(e) => { e.currentTarget.style.borderColor = T.accent; e.currentTarget.style.background = "#20242d"; }}
                    onMouseLeave={(e) => { e.currentTarget.style.borderColor = T.line; e.currentTarget.style.background = T.panel; }}
                  >
                    <div>
                      <div style={{ fontWeight: 600, fontSize: 14, display: "flex", alignItems: "center", gap: 8 }}>
                        {s.title}
                        <span style={{ fontSize: 10, color: T.text3, background: "#191c22", border: `1px solid ${T.lineSoft}`, padding: "1px 7px", borderRadius: 8 }}>进入 →</span>
                      </div>
                      <div style={{ fontSize: 12, color: T.text3, marginTop: 4 }}>起点：{s.time_origin} · 创建：{s.created_at} · 更新：{s.updated_at}</div>
                    </div>
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        removeStory(s.id, s.title);
                      }}
                      style={{ color: T.danger, background: "transparent", border: "none", cursor: "pointer", fontSize: 12.5, padding: "4px 10px" }}
                    >
                      删除
                    </button>
                  </div>
                ))}
              </div>
            </div>
          )
  );
}
