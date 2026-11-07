// 滚动条「平时隐形、动起来才显形、停手 3 秒淡出」（2026-11-07，用户要去）。
//
// 为什么要 CSS class + 这个 hook：
//   · global.css 里给 * 设了 `scrollbar-color`，一旦设了标准属性，Chromium 就**忽略**
//     `::-webkit-scrollbar` 那套定制（这也是第一版「没效果」的真实原因）；
//     所以隐形的正确做法是给元素本身切 `scrollbar-color: transparent`。
//   · class 名统一用 `dw-soft` / `dw-scrolling`，样式写在 App 的全局 <style> 里。

import { useEffect, type RefObject } from "react";

export function useSoftScrollbar(ref: RefObject<HTMLElement | null>, idleMs = 3000) {
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    let timer: number | undefined;
    const wake = () => {
      el.classList.add("dw-scrolling");
      window.clearTimeout(timer);
      timer = window.setTimeout(() => el.classList.remove("dw-scrolling"), idleMs);
    };
    el.addEventListener("scroll", wake, { passive: true });
    el.addEventListener("mousedown", wake);
    el.addEventListener("mouseup", wake);
    return () => {
      window.clearTimeout(timer);
      el.removeEventListener("scroll", wake);
      el.removeEventListener("mousedown", wake);
      el.removeEventListener("mouseup", wake);
    };
  }, [ref, idleMs]);
}
