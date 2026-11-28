import type { AppView, Story } from "../types";
import { T } from "../theme";
import { Icon } from "../components/icons";

export type AppHeaderProps = {
  currentStory: Story | null;
  view: AppView;
  timelineNodeCount: number;
  messageCount: number;
  storyCount: number;
  modelName: string;
  onExport: () => void;
  onBackToStories: () => void;
};

export function AppHeader(props: AppHeaderProps) {
  const { currentStory, view, timelineNodeCount, messageCount, storyCount, modelName, onExport, onBackToStories } = props;
  return (
    <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: 12, padding: "0 20px", height: 52, borderBottom: `1px solid ${T.line}`, background: T.panel, flexShrink: 0 }}>
      <div>
        {currentStory ? (
          <>
            <div style={{ fontSize: 14, fontWeight: 600 }}>{currentStory.title}</div>
            <div style={{ fontSize: 11.5, color: T.text3, marginTop: 2 }}>时间起点：{currentStory.time_origin} · 节点 {timelineNodeCount} · 消息 {messageCount}</div>
          </>
        ) : (
          <>
            <div style={{ fontSize: 14, fontWeight: 600 }}>{view === "home" ? "首页" : view === "stories" ? "故事列表" : "AI 剧情创作"}</div>
            <div style={{ fontSize: 11.5, color: T.text3, marginTop: 2 }}>{view === "home" ? (storyCount ? `${storyCount} 个故事 · 从上次停下的地方继续` : "还没有故事 · 先建一个") : view === "stories" ? `${storyCount} 个故事 · 点击卡片进入导演工作台` : "从左侧导航选择功能"}</div>
          </>
        )}
      </div>
      {currentStory && (
        <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 8 }}>
          <span style={{ display: "inline-flex", alignItems: "center", gap: 5, height: 26, padding: "0 10px", borderRadius: 13, background: T.panel2, border: `1px solid ${T.line}`, fontSize: 11.5, color: T.text2 }}>
            <span style={{ width: 6, height: 6, borderRadius: "50%", background: T.ok }} />{modelName ? `模型：${modelName}` : "模型未配置"}
          </span>
          <button onClick={onExport} style={{ display: "inline-flex", alignItems: "center", gap: 5, height: 26, padding: "0 10px", borderRadius: 13, background: T.panel2, border: `1px solid ${T.line}`, fontSize: 11.5, color: T.text2, cursor: "pointer" }}><Icon name="download" size={12} /> 导出</button>
          <button onClick={onBackToStories} style={{ padding: "5px 12px", border: `1px solid ${T.line}`, borderRadius: 8, background: T.panel2, color: T.text2, cursor: "pointer", fontSize: 12 }}>← 返回故事库</button>
        </div>
      )}
    </div>
  );
}
