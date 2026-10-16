//! 生成任务的取消机制（第五批「主生成链路」修复 · 依据《现状说明》5.2）
//!
//! ## 原文
//!
//! > **主生成链路没有超时、没有重试、没有取消**……请求可以永远不返回 ——
//! > 界面永远停在「正在生成」，而且没有任何取消按钮 / 取消命令。
//!
//! ## 设计
//!
//! 一次生成 = 一个 `request_id`（前端生成，用 UUID）。注册表只存"是否被取消"这一个布尔位：
//!
//! - 前端点「停止」→ `cancel_generation(request_id)` → 把该 id 标记为已取消
//! - 流式循环**每收到一个 chunk 就检查一次** → 已取消则中止，并给前端发
//!   `chat-cancelled` 事件（让界面把已生成的部分标成"已停止"而不是"已完成"）
//!
//! 为什么不用 `tokio::select!` + `CancellationToken`：
//! 现有循环是 `while let Some(chunk) = stream.next().await`，加一个 `AtomicBool` 检查
//! **改动最小、最好审**；而且取消延迟上限就是"下一个 chunk 到达的时间"，
//! 配合流式空闲超时（见 `generate_stream`），最坏情况也能在超时时间内退出。
//!
//! 表是**进程级**的（一次运行内有效）：进程退出即清空，不需要持久化。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

type Flag = std::sync::Arc<AtomicBool>;

fn registry() -> &'static Mutex<HashMap<String, Flag>> {
    static R: OnceLock<Mutex<HashMap<String, Flag>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 登记一次生成，返回它的取消标志。
///
/// **同一个 `request_id` 重复登记会复用已有标志** —— 前端重试同一个 id 时不会互踩。
pub fn register(request_id: &str) -> Flag {
    let mut m = registry().lock().unwrap_or_else(|e| e.into_inner());
    m.entry(request_id.to_string())
        .or_insert_with(|| std::sync::Arc::new(AtomicBool::new(false)))
        .clone()
}

/// 请求取消。返回"是否找到了这个任务"（没找到说明它已经结束了）。
pub fn cancel(request_id: &str) -> bool {
    let m = registry().lock().unwrap_or_else(|e| e.into_inner());
    match m.get(request_id) {
        Some(f) => {
            f.store(true, Ordering::SeqCst);
            true
        }
        None => false,
    }
}

/// 结束登记（**必须**在生成结束/失败/取消时调用，否则表会一直涨）。
pub fn finish(request_id: &str) {
    let mut m = registry().lock().unwrap_or_else(|e| e.into_inner());
    m.remove(request_id);
}

/// 是否已被取消
pub fn is_cancelled(flag: &Flag) -> bool {
    flag.load(Ordering::SeqCst)
}

/// 当前在跑的任务数（供诊断/测试）
pub fn active_count() -> usize {
    registry().lock().unwrap_or_else(|e| e.into_inner()).len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_cancel_finish_roundtrip() {
        let id = "test-cancel-1";
        let f = register(id);
        assert!(!is_cancelled(&f), "刚登记时不该是已取消");
        assert!(cancel(id), "取消应能找到任务");
        assert!(is_cancelled(&f), "取消后标志应可见");
        finish(id);
        assert!(!cancel(id), "结束登记后不该再找得到");
    }

    #[test]
    fn same_id_reuses_flag() {
        let id = "test-cancel-2";
        let a = register(id);
        let b = register(id);
        assert!(cancel(id));
        assert!(is_cancelled(&a) && is_cancelled(&b), "同一个 id 应共享标志");
        finish(id);
    }

    #[test]
    fn unknown_id_is_not_an_error() {
        assert!(!cancel("never-registered"));
    }

    #[test]
    fn finish_removes_entry() {
        let before = active_count();
        let id = "test-cancel-3";
        register(id);
        assert_eq!(active_count(), before + 1);
        finish(id);
        assert_eq!(active_count(), before, "结束后不该留残留");
    }
}
