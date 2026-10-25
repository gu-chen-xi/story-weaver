// 生成流（C1，2026-10-18）：从 `views/StoryInput.tsx` 抽出来的"发消息 / 停生成 / 流式事件"。
//
// 为什么抽：
//   * StoryInput 有 453 行、43 个 props，其中一半逻辑都是这一条链路（存用户消息 → 监听 4 个事件 →
//     调流式 API → 收尾刷新 + 补摘要 + 演进分析 + 评估）；
//   * 视图和链路混在一起，任何一处改动都要在 400 多行 JSX 里找；
//   * 抽成 hook 后，StoryInput 只剩"渲染 + 本页小菜单状态"，链路可以单独读、单独改。
//
// **行为完全不变**：本文件里的代码是从 StoryInput 里**逐字搬来的**，只把外部依赖改成 `deps.xxx`。
// 五批修复的坑都还在（见下面各处注释）：连点并发、requestId 过滤、孤儿用户消息撤销、
// 监听器卸载清理、失败回填输入框。
//
// 依赖注入而不是自己取 state：这个链路要读写 App/会话层的十几个东西（消息、节点、角色、开关…），
// 自己再取一遍会变成两份状态源。调用方（StoryInput）把它们原样传进来即可。

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Dispatch, SetStateAction } from "react";
import { DIRECTOR_TAG, SENSITIVE_WORDS } from "../constants";
import { nowStamp } from "../lib/time";
import type { Story } from "../types";
import { EVALUATE_ID } from "../plugins/builtin/evaluate/manifest";

export type GenMode = "旁白" | "对话";

export type GenMessage = {
  id: number;
  role: string;
  content: string;
  mode: string;
  created_at: string;
};

/// 这个链路需要外部提供的一切（全部来自 App / useStorySession / StoryInput 的 props）
export type UseGenerationDeps = {
  currentStory: Story | null;
  node: { id: number; date_label: string } | null;

  /// 输入框内容与写回（失败时要把文字还给用户）
  input: string;
  setInput: Dispatch<SetStateAction<string>>;

  setMsg: Dispatch<SetStateAction<string>>;

  navMode: GenMode;
  /// 待发导演指令（一次性）
  directors: string[];
  /// 导演指令被软件锁住（AI 扮演者 → 用户扮演者 强度 = 0）：
  /// 已排队但没发出去的也**不发**（判定见 lib/directorGate.ts）
  directorBlocked: boolean;
  updateDirectors: (list: string[]) => void;

  sensitiveFilter: boolean;
  myRole: string;
  aiRole: string;
  constraints: string[];
  lenLevel: "简单" | "详细";
  memoryCtx: boolean;
  recallDay: number | null;

  /// 本次请求 id（用于"正在生成中"的互斥判断）
  genRequestRef: { current: string | null };
  setGenRequestId: Dispatch<SetStateAction<string | null>>;
  /// 卸载时注销监听的清理函数
  activeCleanupRef: { current: (() => void) | null };

  setMessages: Dispatch<SetStateAction<GenMessage[]>>;
  loadTimelineAndMessages: (storyId: number, nodeId?: number, branchOverride?: string | null) => Promise<void>;
  loadChars: (storyId: number) => Promise<void>;
  loadDebug: () => Promise<void>;
  loadProposals: () => Promise<void>;
  loadEvals: () => Promise<void>;
  backfillSummaries: (storyId: string, currentNodeId: number) => Promise<void>;
  autoSummarizeToday: (storyId: string, nodeId: number) => Promise<void>;
  autoAnalyze: boolean;
  pluginOn: (id: string) => boolean;
  /// v3.4：导演监视器「当前角色卡」上的焦点角色名。
  /// 用途：示例对话的第③优先级（①@提到 ②AI扮演 ③它 ④不给）。
  panelFocusName: string;
};

export type UseGenerationResult = {
  sendMessage: () => Promise<void>;
  stopGeneration: () => Promise<void>;
};

export function useGeneration(deps: UseGenerationDeps): UseGenerationResult {
  const {
    currentStory, node, input, setInput, setMsg, navMode, directors, directorBlocked, updateDirectors,
    sensitiveFilter, myRole, aiRole, constraints, lenLevel, memoryCtx, recallDay,
    genRequestRef, setGenRequestId, activeCleanupRef, setMessages,
    loadTimelineAndMessages, loadChars, loadDebug, loadProposals, loadEvals,
    backfillSummaries, autoSummarizeToday, autoAnalyze, pluginOn, panelFocusName,
  } = deps;

  const sendMessage = async () => {
    if (!currentStory || !node || !input.trim()) return;
    // **正在生成中不允许再发**（《现状说明》5.3：连点两次会并发两条流 → 正文串行/重复）
    if (genRequestRef.current) { setMsg("⚠ 正在生成中，请等它结束，或点「⏹ 停止」"); return; }
    const text = input.trim();
    if (sensitiveFilter) {
      const hit = SENSITIVE_WORDS.find((w) => text.includes(w));
      if (hit) { setMsg("🛡 输入包含敏感词，已拦截：" + hit); return; }
    }
    // 导演指令：仅沉浸对话模式，一次性——附加到本条消息开头，发完清空。
    // v3.7.0：强度闸门锁住时**已排队的也不发**（不然闸门形同虚设：
    // 用户可能先排队、再去改关系，队列会一直留着）。
    const pendingDirectors =
      navMode === "对话" && !directorBlocked ? directors.filter((d) => d.trim()) : [];
    if (!text && pendingDirectors.length === 0) return;
    // 解析 #标签（元指令，从剧情正文中剥离）与 @角色（本次聚焦，仅注入提示词）
    const tagRe = /#([^\s#，。、！？]+)/g;
    const tags = (text.match(tagRe) || []).map((t) => t.slice(1)).join(",");
    const stripped = tags ? text.replace(tagRe, "").replace(/\s+/g, " ").trim() : text;
    const focus = text.match(/@([^\s@，。、！？]+)/)?.[1] || "";
    // 指令在正文之前，用空行分隔（后端按同一前缀解析并剥离）
    const content = pendingDirectors.length > 0
      ? `${DIRECTOR_TAG}${pendingDirectors.join("\n")}${stripped ? "\n\n" + stripped : ""}`
      : stripped;
    setInput("");
    if (pendingDirectors.length > 0) updateDirectors([]);   // 一次性：发完即清空待发列表

    // 声明在 try 之外：catch 里要用它撤掉"孤儿用户消息"
    let userMsgId: number | undefined;
    try {
      // 存用户消息（**记下 id**：请求失败时要把这条"孤儿用户消息"撤掉，
      // 否则重发会在历史里留下两份一样的输入 —— 用户实测反馈）
      userMsgId = await invoke<number>("create_message", {
        storyId: String(currentStory.id),
        nodeId: node.id,
        role: "user",
        content,
        mode: navMode === "旁白" ? "narr" : "chat", // 用户消息记录发送时模式，导出按模式过滤
      });
      await loadTimelineAndMessages(currentStory.id, node.id);

      // 准备一个空的 AI 气泡，逐字追加
      const placeholderId = -Date.now();
      setMessages((prev) => [...prev, {
        id: placeholderId,
        node_id: node.id,
        role: "assistant",
        content: "",
        mode: "chat",
        created_at: nowStamp(),
      }]);

      // 第五批：本次请求的唯一 id（后端用它做取消登记）
      // `randomUUID` 在部分 WebView / 非安全上下文里不存在 —— 用**带类型的可选读取**
      // 而不是 `as any`（后者会让整个表达式的类型检查失效）
      const webCrypto = globalThis.crypto as (Crypto & { randomUUID?: () => string }) | undefined;
      const requestId = typeof webCrypto?.randomUUID === "function"
        ? webCrypto.randomUUID()
        : `req-${Date.now()}-${Math.random().toString(16).slice(2)}`;
      genRequestRef.current = requestId;
      setGenRequestId(requestId);

      // 监听流式事件
      // 第五批（《现状说明》5.3）：**事件带 requestId，只认自己那次**。
      // 原来的事件是全局广播、不带请求编号 —— 多窗口 / 旧监听器残留 / 连发两次时，
      // 同一批增量会被多个气泡同时追加，表现为"正文重复 / 串行"。
      const isMine = (p: { requestId?: string }) => !p.requestId || p.requestId === requestId;
      const unlistenToken = await listen<{ requestId?: string; delta: string }>("chat-token", (e) => {
        if (!isMine(e.payload)) return;   // 不是本次请求的增量 → 忽略
        let delta = e.payload.delta;
        if (sensitiveFilter) delta = SENSITIVE_WORDS.reduce((s, w) => s.split(w).join("***"), delta);
        setMessages((prev) => prev.map((m) =>
          m.id === placeholderId ? { ...m, content: m.content + delta } : m
        ));
      });
      const unlistenDone = await listen<{ requestId?: string }>("chat-done", async (e) => {
        if (!isMine(e.payload)) return;   // 别的请求的结束事件 → 不理它
        finishListeners();
        await loadTimelineAndMessages(currentStory.id, node.id);
        await loadChars(currentStory.id); // M5: 回复后刷新角色状态卡
        await loadDebug(); // 刷新当前节点的 Token 预算（基于请求记录）
        // D 子项：回复完成后 ① 补齐更早那些天 ② 当天到量则自动刷当天摘要（都不阻塞界面）
        backfillSummaries(String(currentStory.id), node.id).then(() =>
          autoSummarizeToday(String(currentStory.id), node.id)
        );
        // 第三批：回复后**静默**跑一次演进分析（纯本地规则、不调 API；失败不影响写作）
        if (autoAnalyze) {
          invoke<[number, number]>("auto_analyze_evolution", { storyId: String(currentStory.id), nodeId: node.id })
            .then(async ([nProposals, nApplied]) => {
              if (nProposals > 0 || nApplied > 0) {
                setMsg(`📋 演进分析：新增提案 ${nProposals} 条${nApplied > 0 ? `，自动应用 ${nApplied} 组` : ""}（见「演进提案」）`);
                await loadProposals();
              }
            })
            .catch((e) => console.error("自动演进分析失败（不影响写作）：", e));
        }
        // 第三批 ④：回复后自动评估（默认关，因为要多调一次 API；手动评估不受此限）。
        // 拿不到新消息 id（setMessages 是异步的），所以查一次"本节点最后一条 AI 回复"。
        if (pluginOn(EVALUATE_ID)) {
          invoke<{ id: number; role: string }[]>("list_messages", { storyId: String(currentStory.id), nodeId: node.id })
            .then((list) => {
              const lastAi = [...list].reverse().find((m) => m.role === "assistant");
              if (!lastAi) return;
              return invoke<{ total: number }>("evaluate_reply", { storyId: String(currentStory.id), messageId: lastAi.id })
                .then((r) => {
                  setMsg(`📊 本轮回复评分：${r.total} / 5（见「插件 / 扩展 › 评估模型」）`);
                  loadEvals();
                });
            })
            .catch((e) => console.error("自动评估失败（不影响写作）：", e));
        }
      });
      // 收尾共用：注销四个监听器 + 从库里重新读（这样"部分生成"的标记也能显示出来）
      const finishListeners = () => {
        unlistenToken();
        unlistenDone();
        unlistenError();
        unlistenCancelled();
        if (genRequestRef.current === requestId) {
          genRequestRef.current = null;
          setGenRequestId(null);
        }
      };
      // **停止**（第五批新增）：后端把已生成的部分存下来并标记，这里重读显示
      const unlistenCancelled = await listen<{ requestId?: string; detail: string }>("chat-cancelled", async (e) => {
        if (!isMine(e.payload)) return;
        finishListeners();
        await loadTimelineAndMessages(currentStory.id, node.id);
        setMsg("⏹ 已停止生成 —— 已生成的部分保留在消息里（带未完成标记）");
        console.debug("[生成已停止]", e.payload.detail);
      });
      const unlistenError = await listen<{ requestId?: string; detail: string; partial?: string }>("chat-error", async (e) => {
        if (!isMine(e.payload)) return;
        const hadPartial = !!e.payload.partial;
        finishListeners();
        if (hadPartial) {
          // 已经收到一部分 → 后端把它存下来了（带"未完成"标记），重读并提示
          await loadTimelineAndMessages(currentStory.id, node.id);
          setMsg("⚠ 生成中断（已保留收到的部分）：" + e.payload.detail);
        } else {
          setMessages((prev) => prev.filter((m) => m.id !== placeholderId));
          setMsg("❌ " + e.payload.detail);
        }
      });

      // 第五批（《现状说明》5.3）：**把清理登记到 ref，组件卸载时也能注销**。
      // 原实现只在 done/error 里注销；用户在生成中切走页面/关故事时监听器会留到进程结束。
      activeCleanupRef.current = finishListeners;

      // 调流式 API
      await invoke("generate_story_reply_stream", {
        storyId: String(currentStory.id),
        nodeId: node.id,
        userText: content,
        mode: navMode === "旁白" ? "narr" : "chat",
        myRole,
        aiRole,
        focusRole: focus,
        // v3.4：示例对话第③优先级（导演监视器焦点角色）；空串也让后端按"没有"处理
        exampleOwner: panelFocusName || null,
        tags,
        constraints: constraints.map((cst, i) => `${i + 1}. ${cst}`).join("\n"),
        lengthLevel: lenLevel,
        memory: memoryCtx,
        recallDay: recallDay ?? null,
        requestId,
      });
    } catch (e) {
      // **失败要撤销这条用户消息并把文字还给输入框**（用户实测反馈）：
      // 存用户消息发生在调 API **之前**，请求失败后那条消息会留在历史里成为"孤儿"，
      // 用户重发就会看到两份一样的输入。
      // 用消息 id 精确删除（不靠文本匹配），并把文字放回输入框，避免还要重打。
      try {
        if (userMsgId != null) {
          await invoke("delete_message", { storyId: String(currentStory.id), messageId: userMsgId });
          await loadTimelineAndMessages(currentStory.id, node.id);
        }
      } catch (e2) {
        console.error("撤销未发送的用户消息失败（不影响其它功能）：", e2);
      }
      setInput(stripped);
      setMsg("❌ 发送失败（已撤回这条消息，文字已放回输入框，可直接重发）: " + String(e));
    } finally {
      genRequestRef.current = null;
      setGenRequestId(null);
    }
  };

  const stopGeneration = async () => {
    const id = genRequestRef.current;
    if (!id) { setMsg("当前没有正在进行的生成"); return; }
    setMsg("⏹ 正在停止…");
    try {
      const found = await invoke<boolean>("cancel_generation", { requestId: id });
      if (!found) setMsg("⚠ 生成已经结束了");
    } catch (e) {
      setMsg("❌ 停止失败：" + String(e));
    }
  };

  return { sendMessage, stopGeneration };
}
