// 滚动条「平时隐形、动起来才显形、停手 3 秒淡出」（2026-11-07，用户要去）。
//
// 两个坑（都是实测踩到的）：
//   ① global.css 给 `*` 设了 `scrollbar-color` —— 一旦有标准属性，Chromium 就**忽略**
//      `::-webkit-scrollbar` 那套定制。所以「隐形」必须用 `scrollbar-color: transparent`，
//      写在 App 的全局 <style> 里（.dw-soft / .dw-scrolling）。
//   ② 目标元素可能是**条件渲染**的（导演监视器没打开故事时整块不渲染）——
//      挂载那一帧 ref 还是 null，effect 又只跑一次，监听就永远挂不上（表现为滚动条彻底消失）。
//      所以这里带「重试直到元素出现」的小循环，外加 `deps` 让调用方按需重挂。

import { useEffect, type RefObject } from "react";

export function useSoftScrollbar(
  ref: RefObject<HTMLElement | null>,
  idleMs = 3000,
  deps: unknown[] = [],
) {
  useEffect(() => {
    let cleanup: (() => void) | null = null;
    let cancelled = false;
    let raf = 0;
    let tries = 0;
    let timer: number | undefined;

    const attach = (el: HTMLElement) => {
      const wake = () => {
        el.classList.add("dw-scrolling");
        window.clearTimeout(timer);
        timer = window.setTimeout(() => el.classList.remove("dw-scrolling"), idleMs);
      };
      el.addEventListener("scroll", wake, { passive: true });
      el.addEventListener("mousedown", wake);
      el.addEventListener("mouseup", wake);
      cleanup = () => {
        window.clearTimeout(timer);
        el.removeEventListener("scroll", wake);
        el.removeEventListener("mousedown", wake);
        el.removeEventListener("mouseup", wake);
      };
    };

    const tick = () => {
      if (cancelled) return;
      const el = ref.current;
      if (el) {
        attach(el);
        return;
      }
      // 元素还没挂上（条件渲染）：最多等 5 秒，之后放弃，避免空转烧 CPU
      if (++tries > 300) return;
      raf = requestAnimationFrame(tick);
    };
    tick();

    return () => {
      cancelled = true;
      cancelAnimationFrame(raf);
      cleanup?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ref, idleMs, ...deps]);
}
