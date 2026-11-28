// 统一错误上报（纯逻辑 + 可注入 sink）。
//
// reportError：记日志 + 默认发 T1 toast；silent 时返回文本给页面内 T2 使用。
// expectedError：明确可忽略的预期失败，必须写原因。
// 这里不包装 invoke，也不返回默认值 —— 错误由最靠近 UI 的调用方决定怎么展示。

export type ErrorLevel = "error" | "warn";

export type ReportOptions = {
  silent?: boolean;
  level?: ErrorLevel;
  message?: string;
};

type ErrorSink = (text: string) => void;

let sink: ErrorSink | null = null;

export function bindErrorSink(next: ErrorSink | null): () => void {
  sink = next;
  return () => {
    if (sink === next) sink = null;
  };
}

export function errorText(error: unknown): string {
  if (typeof error === "string") return error.trim() || "未知错误";
  if (error instanceof Error) return (error.message || error.name || "未知错误").trim();
  if (error && typeof error === "object") {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string" && message.trim()) return message.trim();
    try {
      const json = JSON.stringify(error);
      if (json && json !== "{}") return json;
    } catch {
      // 循环引用等对象退回 String()
    }
  }
  return String(error ?? "未知错误");
}

export function reportError(scope: string, error: unknown, opts: ReportOptions = {}): string {
  console.error(`[${scope}]`, error);
  const detail = errorText(error);
  const label = opts.message ?? scope;
  const prefix = (opts.level ?? "error") === "warn" ? "\u26A0" : "\u274C";
  const text = `${prefix} ${label}：${detail}`;
  if (!opts.silent) sink?.(text);
  return text;
}

export function expectedError(reason: string, error: unknown): void {
  console.debug(`[expected] ${reason}`, error);
}