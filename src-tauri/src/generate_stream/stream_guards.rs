// 从 `generate_stream.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `generate_stream::stream_guards` —— 因为 `generate_stream.rs` + `generate_stream/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `generate_stream`，一行都不用改写。

const SRC: &str = include_str!("../generate_stream.rs");

/// 超时常量必须存在且被真正使用（不能只定义不用）
#[test]
fn timeouts_are_defined_and_used() {
    assert!(SRC.contains("GEN_CONNECT_TIMEOUT_SECS"), "缺少连接/首字节超时常量");
    assert!(SRC.contains("GEN_STREAM_IDLE_TIMEOUT_SECS"), "缺少流式空闲超时常量");
    // send 必须被 timeout 包住
    let send_at = SRC.find(".send(),").or_else(|| SRC.find(".send()")).expect("找不到 send()");
    let before = &SRC[..send_at];
    assert!(
        before.rfind("tokio::time::timeout").map(|i| send_at - i < 800).unwrap_or(false),
        "请求发送必须包在 tokio::time::timeout 里（否则上游半开时永远不返回）"
    );
    // 客户端必须带 connect_timeout
    assert!(SRC.contains(".connect_timeout("), "HTTP 客户端必须设置 connect_timeout");
    // 注意：这个字符串**不能直接字面量写在本测试里** —— 否则断言会匹配到自己的源码。
    // 用拼接构造出待查串，再按行扫（注释行除外）。
    let forbidden = ["reqwest::Client", "::new()"].concat();
    for (i, line) in SRC.lines().enumerate() {
        let code = line.trim();
        if code.starts_with("//") {
            continue;
        }
        // 拼接过的 needle 不会出现在本行自身，所以不会自匹配
        assert!(
            !code.contains(&forbidden),
            "第 {} 行又用回了无超时的客户端构造：{code}",
            i + 1
        );
    }
}

/// 循环里必须有取消检查 + 空闲超时
#[test]
fn stream_loop_checks_cancel_and_idle_timeout() {
    assert!(SRC.contains("gen_cancel::is_cancelled"), "流式循环缺少取消检查");
    assert!(SRC.contains("GEN_STREAM_IDLE_TIMEOUT_SECS"), "流式循环缺少空闲超时引用");
    assert!(SRC.contains("crate::gen_cancel::register"), "生成开始必须登记取消标志");
    assert!(SRC.contains("crate::gen_cancel::finish"), "生成结束必须注销取消标志");
}

/// 失败路径必须发事件（否则前端监听器永不注销 —— §5.3）
#[test]
fn failure_paths_emit_events() {
    for ev in ["chat-error", "chat-done", "chat-cancelled"] {
        assert!(SRC.contains(ev), "缺少事件 {ev}（前端要靠它注销监听器）");
    }
    // 请求失败分支（send 之前那次 return）必须也发 chat-error
    let fail_at = SRC.find("请求失败: {e}").expect("找不到请求失败分支");
    let tail = &SRC[fail_at..(fail_at + 900).min(SRC.len())];
    assert!(tail.contains("chat-error"), "send 失败的分支必须发 chat-error 事件");
}

/// 读到一半失败/被取消时：必须**保留已收到的部分并标记**，不能当成完整回复
#[test]
fn partial_stream_keeps_and_marks_content() {
    assert!(SRC.contains("stream_err"), "缺少读流错误的记录变量");
    assert!(SRC.contains("cancelled"), "缺少取消标记");
    assert!(
        SRC.contains("本段生成未完成"),
        "部分内容入库时必须带明确标记（否则用户拿到被截断的正文却不知道）"
    );
}

/// 第五批（《现状说明》5.3）：**每个流式事件都必须带 requestId**。
///
/// 原来的事件是全局广播、不带请求编号 —— 多窗口 / 旧监听器残留 / 连发两次时，
/// 同一批增量会被多个气泡同时追加（表现为"正文重复 / 串行"）。
/// 这条守着"以后新增事件别忘了带编号"。
#[test]
fn every_emit_carries_request_id() {
    // **只扫生产代码**（本测试模块之前的部分）—— 否则断言里那个字面量会匹配到自己。
    let prod_end = SRC.find("mod stream_guards").expect("找不到 stream_guards 模块起点");
    let prod = &SRC[..prod_end];
    let all: Vec<&str> = prod.lines().collect();
    let mut checked = 0;
    for (i, line) in all.iter().enumerate() {
        if !line.contains(".emit(") {
            continue;
        }
        checked += 1;
        let seg: String = all[i..(i + 5).min(all.len())].join("\n");
        assert!(
            seg.contains("requestId"),
            "第 {} 行的 emit 没有带 requestId：\n{seg}",
            i + 1
        );
    }
    assert!(checked >= 8, "应至少检查到 8 处 emit，实际 {checked}");
}
