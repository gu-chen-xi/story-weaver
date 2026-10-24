// 示例插件的面板（模板）。
//
// 约定：
//   * props 只有 `T`（主题）+ 你自己需要的字段；
//   * 面板需要的**宿主数据**由 `index.tsx` 的 `renderPanel` 从 hostData 里挑出来传进来 ——
//     面板自己不要 import App / hooks，也不要读全局状态（否则就没法单测、也没法换宿主）。
//   * 不要在这里写"确认删除""是否覆盖"这类二次确认之外的破坏性操作；
//     需要落盘的写操作一律走 `invoke()` 到后端。

import type { PluginCardTokens } from "../PluginCard";

export type TemplatePanelProps = {
  T: PluginCardTokens;
  /** 有多少条数据可展示（示例字段，换成你自己的） */
  count: number;
};

export function TemplatePanel({ T, count }: TemplatePanelProps) {
  return (
    <div style={{ border: `1px solid ${T.line}`, borderRadius: 10, padding: "10px 12px", background: T.panel }}>
      <div style={{ fontSize: 12, color: T.text2, marginBottom: 6 }}>
        这里放插件自己的界面。当前有 <b style={{ color: T.text1 }}>{count}</b> 条数据。
      </div>
      <div style={{ fontSize: 11, color: T.text3, lineHeight: 1.7 }}>
        面板拿到的只是宿主传进来的数据（见 <code>index.tsx</code> 的 renderPanel）；
        需要写操作就 <code>invoke()</code> 到后端命令，不要直接改数据库。
      </div>
    </div>
  );
}
