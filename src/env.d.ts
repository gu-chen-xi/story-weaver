/// <reference types="vite/client" />
// 让 TypeScript 认识 `.css` 这类**副作用导入**（以及 Vite 的 import.meta.env）。
//
// 起因（2026-10-18）：新增 src/global.css 后，`import "./global.css";` 报
// TS2882 "Cannot find module or type declarations for side-effect import" ——
// 项目此前没有任何全局样式文件，所以也从来没有 d.ts。Vite 自带的 client.d.ts
// 已经声明了 `*.css` 模块，加这一行引用即可。
