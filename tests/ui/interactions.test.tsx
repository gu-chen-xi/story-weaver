// 交互层测试（2026-11-23，jsdom + Testing Library）。
//
// 这一层补的是「点一下会发生什么」：按钮回调、侧栏切页/重复点刷新、弹窗开关与折叠、
// 双池对比的绿/黄/红三色护栏。上一层的 `tests/uiRender.test.cjs` 只管"渲染得出来"。
//
// 组件里唯一碰 IPC 的是 `@tauri-apps/api/core` 的 invoke —— 已被 `setup.ts` 统一 mock，
// 每个用例自己决定它返回什么。

import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { HomePage } from "../../src/views/HomePage";
import { Sidebar } from "../../src/views/Sidebar";
import { MemoryModal } from "../../src/views/MemoryModal";
import { PromptPoolCompare } from "../../src/views/PromptPoolCompare";
import { ConfirmProvider } from "../../src/confirm";

const story = (id: number, title: string) => ({
  id,
  title,
  time_origin: "2026-01-01",
  prologue: "",
  created_at: "2026-11-01 10:00:00",
  updated_at: "2026-11-19 10:00:00",
});

const memory = (id: string, chars: string[], body: string) => ({
  id,
  kind: "confession",
  label: "表白",
  day_label: "第 3 天",
  branch: null,
  importance: 90,
  body,
  created_at: "2026-11-19 10:00:00",
  characters: chars,
  ai_side: "",
});

beforeEach(() => {
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockResolvedValue(null);
});

describe("首页", () => {
  it("点「继续创作」把最近的故事交给回调，等待期间按钮锁住并在结束后恢复", async () => {
    const user = userEvent.setup();
    let release!: () => void;
    const pending = new Promise<void>((resolve) => {
      release = resolve;
    });
    const onContinue = vi.fn(() => pending);
    const stories = [story(1, "甲"), story(2, "乙")];
    render(<HomePage stories={stories} onContinue={onContinue} onNew={vi.fn()} onAll={vi.fn()} />);

    await user.click(screen.getByRole("button", { name: /继续创作 · 甲/ }));
    expect(onContinue).toHaveBeenCalledTimes(1);
    expect(onContinue).toHaveBeenCalledWith(stories[0]);

    const busyBtn = screen.getByRole("button", { name: "进入中…" }) as HTMLButtonElement;
    expect(busyBtn.disabled).toBe(true);

    release();
    await waitFor(() => {
      expect((screen.getByRole("button", { name: /继续创作 · 甲/ }) as HTMLButtonElement).disabled).toBe(false);
    });
  });

  it("点最近编辑里的第 2 条，会把那一条交给回调", async () => {
    const user = userEvent.setup();
    const onContinue = vi.fn(async () => {});
    const stories = [story(1, "甲"), story(2, "乙")];
    render(<HomePage stories={stories} onContinue={onContinue} onNew={vi.fn()} onAll={vi.fn()} />);
    await user.click(screen.getByRole("button", { name: /乙/ }));
    expect(onContinue).toHaveBeenCalledWith(stories[1]);
  });
});

describe("侧栏", () => {
  const baseProps = (over: Record<string, unknown> = {}) => ({
    complianceAck: true,
    currentStory: story(4, "测试1"),
    loadCloudConfig: vi.fn(async () => {}),
    loadComplianceAck: vi.fn(async () => {}),
    loadDebug: vi.fn(async () => {}),
    loadGenerators: vi.fn(async () => {}),
    loadProposals: vi.fn(async () => {}),
    goHome: vi.fn(),
    pendingGroups: 0,
    setGenProbe: vi.fn(),
    setShowSettings: vi.fn(),
    setView: vi.fn(),
    view: "story" as const,
    navItems: [
      { icon: "folder", label: "故事列表", view: "stories", action: vi.fn() },
      { icon: "calendar", label: "时间线", view: "timeline", sub: true },
      { icon: "clipboard", label: "演进提案", view: "proposals", sub: true },
    ],
    ...over,
  });

  it("点二级入口「时间线」→ setView('timeline')", async () => {
    const user = userEvent.setup();
    const props = baseProps();
    render(<Sidebar {...(props as never)} />);
    await user.click(screen.getByRole("button", { name: /时间线/ }));
    expect(props.setView).toHaveBeenCalledWith("timeline");
  });

  it("重复点「演进提案」每次都刷新（2026-11-05 的要求）", async () => {
    const user = userEvent.setup();
    const props = baseProps();
    render(<Sidebar {...(props as never)} />);
    const btn = screen.getByRole("button", { name: /演进提案/ });
    await user.click(btn);
    await user.click(btn);
    expect(props.setView).toHaveBeenCalledTimes(2);
    expect(props.loadProposals).toHaveBeenCalledTimes(2);
  });

  it("点左上角 Logo → 回首页回调", async () => {
    const user = userEvent.setup();
    const props = baseProps();
    render(<Sidebar {...(props as never)} />);
    await user.click(screen.getByTitle("回到首页"));
    expect(props.goHome).toHaveBeenCalledTimes(1);
  });
});

describe("记忆弹窗", () => {
  const modal = (items: ReturnType<typeof memory>[], onClose = vi.fn()) => (
    <ConfirmProvider>
      <MemoryModal
        storyId="4"
        storyTitle="测试1"
        character="夏婉琴"
        items={items as never}
        onReload={vi.fn(async () => {})}
        onClose={onClose}
        setMsg={vi.fn()}
      />
    </ConfirmProvider>
  );

  it("点 ✕ 关闭调 onClose", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    render(modal([memory("001", ["夏婉琴", "顾宸熙"], "**表白**：他说喜欢她。")], onClose));
    await user.click(screen.getByRole("button", { name: "✕" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("「其它关系组」默认折叠，点一下展开再收起", async () => {
    const user = userEvent.setup();
    render(modal([
      memory("001", ["夏婉琴", "顾宸熙"], "他说喜欢她。"),
      memory("002", ["顾宸熙", "顾母"], "母子吵架。"),
    ]));
    const toggle = screen.getByRole("button", { name: /展开其它关系组/ });
    await user.click(toggle);
    expect(screen.getByRole("button", { name: /收起其它关系组/ })).toBeTruthy();
    expect(screen.getByText(/母子吵架/)).toBeTruthy();
    await user.click(screen.getByRole("button", { name: /收起其它关系组/ }));
    expect(screen.getByRole("button", { name: /展开其它关系组/ })).toBeTruthy();
  });
});

describe("双池对比的三色护栏", () => {
  const pair = (preview: string, real: string) => ({
    ai_role: "夏婉琴",
    my_role: "顾宸熙",
    global: { text: "", chars: 0, tokens: 0 },
    role: { text: "", chars: 0, tokens: 0 },
    tail_global: { text: preview, chars: preview.length, tokens: 0 },
    tail_role: { text: preview, chars: preview.length, tokens: 0 },
    memory_groups: [],
    last_request: { id: 96, node_id: 44, tail_chars: real.length, tail_block: real },
    last_focus: "",
  });

  it("全部命中 → 绿条", async () => {
    vi.mocked(invoke).mockResolvedValue(
      pair("【数值与当下·角色属性】\n- 夏婉琴：体力 80", "【数值与当下】\n- 夏婉琴：体力 80"),
    );
    render(<PromptPoolCompare storyId={4} aiRole="夏婉琴" myRole="顾宸熙" />);
    expect(await screen.findByText(/逐行核对通过/)).toBeTruthy();
  });

  it("名字对得上、数据变过 → 黄条（不算预览出错）", async () => {
    vi.mocked(invoke).mockResolvedValue(
      pair("【数值与当下·角色属性】\n- 夏婉琴：体力 80｜动作：转身", "【数值与当下】\n- 夏婉琴：体力 80｜动作：平静"),
    );
    render(<PromptPoolCompare storyId={4} aiRole="夏婉琴" myRole="顾宸熙" />);
    expect(await screen.findByText(/不算预览出错/)).toBeTruthy();
  });

  it("名字都对不上 → 红条（真漂移）", async () => {
    vi.mocked(invoke).mockResolvedValue(
      pair("【数值与当下·关系强度】\n- 顾宸熙 → 夏婉琴：强度 40", "【数值与当下】\n- 夏婉琴 → 顾宸熙：强度 0"),
    );
    render(<PromptPoolCompare storyId={4} aiRole="夏婉琴" myRole="顾宸熙" />);
    expect(await screen.findByText(/可能已经和真实装配漂移|找不到/)).toBeTruthy();
  });
});
