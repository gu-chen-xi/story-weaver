import { defineConfig } from "vitest/config";

// 交互层测试（2026-11-23 加）：jsdom + Testing Library。
// 纯逻辑/渲染的 node:test 那套不动（`npm run test:unit`），这里是第二套跑法。
export default defineConfig({
  test: {
    environment: "jsdom",
    include: ["tests/ui/**/*.test.tsx"],
    setupFiles: ["tests/ui/setup.ts"],
    globals: false,
    restoreMocks: true,
    clearMocks: true,
  },
});
