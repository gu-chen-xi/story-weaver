import { Component, StrictMode, type ErrorInfo, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import "./global.css";
import App from "./App";

/**
 * 全局错误边界。
 *
 * 为什么要它：之前界面一旦抛异常就是**整页白屏**，用户只能反馈"软件崩了"，
 * 拿不到任何线索（实测遇到过一次）。有了它，崩溃时会把错误信息、堆栈、组件栈
 * 直接显示在页面上，可以一键复制 —— 定位问题不需要再来回猜。
 */
class ErrorBoundary extends Component<{ children: ReactNode }, { err: Error | null; info: string }> {
  constructor(props: { children: ReactNode }) {
    super(props);
    this.state = { err: null, info: "" };
  }
  static getDerivedStateFromError(err: Error) {
    return { err, info: "" };
  }
  componentDidCatch(err: Error, info: ErrorInfo) {
    this.setState({ err, info: info.componentStack ?? "" });
    // 同时打到控制台，便于用 F12 复制
    console.error("[织梦机] 界面崩溃：", err, info);
  }
  render() {
    if (!this.state.err) return this.props.children;
    const text = `${this.state.err.name}: ${this.state.err.message}\n\n${this.state.err.stack ?? ""}\n\n组件栈：${this.state.info}`;
    return (
      <div style={{ padding: 24, fontFamily: "system-ui, sans-serif", color: "#e6e8ee", background: "#14161b", minHeight: "100vh" }}>
        <h2 style={{ margin: "0 0 8px", fontSize: 18 }}>⚠ 界面出错了（不是数据丢失）</h2>
        <p style={{ color: "#9aa1b1", fontSize: 13, margin: "0 0 14px", lineHeight: 1.7 }}>
          这是软件的一个缺陷 —— 你的故事数据仍然在库里，没有损坏。<br />
          请把下面的信息复制发给开发者，并说明"点的是什么按钮"。
        </p>
        <button
          onClick={() => { void navigator.clipboard.writeText(text); }}
          style={{ fontSize: 13, padding: "7px 16px", borderRadius: 8, cursor: "pointer",
            border: "1px solid #2a2f3a", background: "#1c2029", color: "#e6e8ee", marginBottom: 12 }}>
          复制错误信息
        </button>
        <button
          onClick={() => this.setState({ err: null, info: "" })}
          style={{ fontSize: 13, padding: "7px 16px", borderRadius: 8, cursor: "pointer",
            border: "1px solid #2a2f3a", background: "#1c2029", color: "#e6e8ee", marginLeft: 8, marginBottom: 12 }}>
          重试（回到界面）
        </button>
        <pre style={{ whiteSpace: "pre-wrap", fontSize: 12, lineHeight: 1.6, background: "#0f1116",
          border: "1px solid #2a2f3a", borderRadius: 8, padding: 14, maxHeight: "60vh", overflow: "auto" }}>
          {text}
        </pre>
      </div>
    );
  }
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </StrictMode>
);
