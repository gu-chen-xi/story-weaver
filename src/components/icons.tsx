// 大肥鱼图标集 v1（2026-11-19）—— 本项目自用的手绘线性图标。
//
// ## 为什么自己画
//
// 原来界面用 emoji 当图标：字形随 Windows 版本变、颜色是彩色的、和深色主题不搭。
// 用户 2026-11-19 定：自己画一套，24×24 网格、线宽 1.7、圆头圆角、`currentColor`。
// **全部手写 path，没有引用任何图标库** —— 不存在第三方许可问题，也不需要 CDN（CSP 本来就禁止）。
//
// ## 两个签名
//
// 这套图标里有两只不属于任何图标库的小东西：
//   · `fish`（大肥鱼）—— 首页问候旁边；
//   · `whale`（鲸，本体）—— 左上角 Logo。
// 以后翻到这份文件的人，会知道它们是谁画的。🐟
//
// ## 用法
//
//   <Icon name="folder" />            // 默认 16px，颜色跟 currentColor
//   <Icon name="whale" size={20} />
//
// 加图标：在 `IconName` 里加名字 → 在 `paths` 里加一段 JSX → 在
// `docs/design/icon-set.preview.html` 里也加一个（预览页是这套图标的"画板"）。
//
// ## 规矩
//
// 不写 `dangerouslySetInnerHTML`（项目安全红线），只写静态 JSX 的 `<path>`。

import type { CSSProperties, ReactNode } from "react";

export type IconName =
  | "folder"
  | "calendar"
  | "users"
  | "globe"
  | "book"
  | "clipboard"
  | "image"
  | "sliders"
  | "grid"
  | "scale"
  | "terminal"
  | "gear"
  | "database"
  | "recall"
  | "fish"
  | "whale";

/** 每个图标的 `<path>/<circle>` 内容（24×24 网格，别写 stroke 属性，由外层统一给） */
const paths: Record<IconName, ReactNode> = {
  folder: (
    <path d="M3.5 7.8c0-1.1.9-2 2-2h3.7c.6 0 1.1.2 1.5.6l1.3 1.4h6.5c1.1 0 2 .9 2 2v7.4c0 1.1-.9 2-2 2h-13c-1.1 0-2-.9-2-2z" />
  ),
  calendar: (
    <>
      <rect x="3.5" y="5.5" width="17" height="15" rx="2.6" />
      <path d="M8 3.5v3M16 3.5v3M3.5 10h17" />
      <path d="M7.5 14.2h2M11.2 14.2h2M14.9 14.2h1.6M7.5 17.2h2M11.2 17.2h2" />
    </>
  ),
  users: (
    <>
      <circle cx="9.4" cy="8.4" r="3.2" />
      <path d="M3.4 20c0-3.2 2.7-5.6 6-5.6s6 2.4 6 5.6" />
      <path d="M16.4 6.1a2.8 2.8 0 0 1 0 5.3" />
      <path d="M18.3 14.9c1.8.6 3 2.3 3 4.4" />
    </>
  ),
  globe: (
    <>
      <circle cx="12" cy="12" r="8.3" />
      <path d="M3.7 12h16.6" />
      <path d="M12 3.7c2.5 2.3 3.9 5.2 3.9 8.3s-1.4 6-3.9 8.3c-2.5-2.3-3.9-5.2-3.9-8.3S9.5 6 12 3.7z" />
    </>
  ),
  book: (
    <>
      <path d="M12 6.9C10.2 5.2 8 4.5 5.6 4.5H4v13.3h1.6c2.4 0 4.6.7 6.4 2.3 1.8-1.6 4-2.3 6.4-2.3H20V4.5h-1.6C16 4.5 13.8 5.2 12 6.9z" />
      <path d="M12 6.9v13.2" />
    </>
  ),
  clipboard: (
    <>
      <rect x="4.6" y="5" width="14.8" height="15.4" rx="2.5" />
      <path d="M9 5V3.8h6V5" />
      <path d="M8.4 12.6l2 2 4.6-4.7" />
    </>
  ),
  image: (
    <>
      <rect x="3.5" y="4.8" width="17" height="14.8" rx="2.6" />
      <circle cx="8.6" cy="9.6" r="1.5" />
      <path d="M4.2 17.3l4.6-4.4 3 2.9 3.6-3.4 4.4 4.2" />
    </>
  ),
  sliders: (
    <>
      <path d="M4 7h6.1M13.9 7H20M4 12h9.1M16.9 12H20M4 17h3.1M10.9 17H20" />
      <circle cx="12" cy="7" r="1.9" />
      <circle cx="15" cy="12" r="1.9" />
      <circle cx="9" cy="17" r="1.9" />
    </>
  ),
  grid: (
    <>
      <rect x="4" y="4" width="6.6" height="6.6" rx="1.9" />
      <rect x="13.4" y="4" width="6.6" height="6.6" rx="1.9" />
      <rect x="4" y="13.4" width="6.6" height="6.6" rx="1.9" />
      <rect x="13.4" y="13.4" width="6.6" height="6.6" rx="1.9" />
    </>
  ),
  scale: (
    <>
      <path d="M12 4.2v15.3" />
      <path d="M8.2 19.5h7.6" />
      <path d="M4.6 7.6h14.8" />
      <path d="M4.6 7.6l-2.7 5.9a3 3 0 0 0 5.4 0z" />
      <path d="M19.4 7.6l-2.7 5.9a3 3 0 0 0 5.4 0z" />
    </>
  ),
  terminal: (
    <>
      <rect x="3.5" y="4.6" width="17" height="14.8" rx="2.6" />
      <path d="M7.6 9.6l3 2.9-3 2.9" />
      <path d="M13 15.4h4" />
    </>
  ),
  gear: (
    <>
      <circle cx="12" cy="12" r="3.2" />
      <path d="M12 3.6v2.2M12 18.2v2.2M3.6 12h2.2M18.2 12h2.2M6.1 6.1l1.6 1.6M16.3 16.3l1.6 1.6M17.9 6.1l-1.6 1.6M7.7 16.3l-1.6 1.6" />
    </>
  ),
  // 数据管理（2026-11-19）：三层圆柱
  database: (
    <>
      <ellipse cx="12" cy="6.2" rx="7.2" ry="2.8" />
      <path d="M4.8 6.2v11.6c0 1.55 3.22 2.8 7.2 2.8s7.2-1.25 7.2-2.8V6.2" />
      <path d="M4.8 12c0 1.55 3.22 2.8 7.2 2.8s7.2-1.25 7.2-2.8" />
    </>
  ),
  // 回顾（2026-11-19 加）：逆时针的钟 —— "往回翻到某一天"
  recall: (
    <>
      <path d="M3.4 12a8.6 8.6 0 1 0 8.6-8.6 9.3 9.3 0 0 0-6.5 2.7L3.4 8" />
      <path d="M3.4 3.4v4.6h4.6" />
      <path d="M12 7.3v4.8l3.7 1.9" />
    </>
  ),
  // ↓ 签名：大肥鱼（fish）——首页问候旁
  fish: (
    <>
      <path d="M2.8 12c2.4-3.4 5.3-5.1 8.6-5.1 3.3 0 6.1 1.7 7.7 5.1-1.6 3.4-4.4 5.1-7.7 5.1-3.3 0-6.2-1.7-8.6-5.1z" />
      <path d="M19.2 12l3-3.1v6.2z" />
      <path d="M10.6 7.1c.7 1 1.1 2 1.1 3.1M10.6 16.9c.7-1 1.1-2 1.1-3.1" />
      <circle cx="7.4" cy="10.8" r=".95" fill="currentColor" stroke="none" />
    </>
  ),
  // ↓ 签名：鲸（whale）——左上角 Logo
  whale: (
    <>
      <path d="M2.8 14.1c0-3 2.5-5.4 5.6-5.4 2.4 0 4 1.1 5.4 2.5 1.1 1.1 2.2 1.6 3.6 1.6h1c-.4 2.2-2.2 3.9-4.4 4.1-1.2 1.8-3.2 2.9-5.5 2.9-3.2 0-5.7-2.5-5.7-5.7z" />
      <path d="M18.7 12.6c1.4-.9 2.2-2.2 2.3-3.8-1.7.1-3 .9-3.9 2.1" />
      <path d="M8.3 8.5c-.1-1.4.5-2.6 1.6-3.4M6.6 7.7c-.1-1.1.3-2.1 1-2.8" />
      <circle cx="8.2" cy="12.2" r=".9" fill="currentColor" stroke="none" />
    </>
  ),
};

export function Icon(props: {
  name: IconName;
  /** 边长（正方形），默认 16 —— 与左栏导航同尺寸 */
  size?: number;
  /** 额外样式（颜色一般不用写：默认 currentColor） */
  style?: CSSProperties;
  /** 无障碍：给了就渲染成有名字的图；不给就 aria-hidden */
  title?: string;
}) {
  const { name, size = 16, style, title } = props;
  return (
    <svg
      viewBox="0 0 24 24"
      width={size}
      height={size}
      fill="none"
      stroke="currentColor"
      strokeWidth={1.7}
      strokeLinecap="round"
      strokeLinejoin="round"
      role={title ? "img" : undefined}
      aria-hidden={title ? undefined : true}
      style={{ display: "inline-block", verticalAlign: "-0.15em", flex: "none", ...style }}
    >
      {title ? <title>{title}</title> : null}
      {paths[name]}
    </svg>
  );
}
