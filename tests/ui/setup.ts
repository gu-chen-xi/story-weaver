// 交互层测试的公共准备（2026-11-23）。
//
// 组件里只有 `@tauri-apps/api/core` 的 `invoke` 会碰 IPC；jsdom 里没有 Tauri，
// 所以统一 mock 掉，具体返回值由各用例自己 `vi.mocked(invoke).mockResolvedValue(...)` 决定。

import { afterEach, vi } from "vitest";
import { cleanup } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => null),
}));

// vitest 的 `globals: false` 下 Testing Library 不会自动清理 DOM —— 不 cleanup 上一条用例的树会留在 document 里，
// 后续 `getByRole` 就会撞到"多个元素"（2026-11-23 本层第一批测试踩到）。
afterEach(() => cleanup());
