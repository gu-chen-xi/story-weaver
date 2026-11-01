// 两套流式协议的**行解析**单元测试（2026-11-01，3.13「本地主对话走原生 /api/chat」）。
//
// 为什么单独测这个：真实流式链路要连上游、跑不了单测；但"这一行怎么读"是纯函数，
// 而它恰恰是最容易改坏的地方 —— 云端 SSE 的 choices[0].delta.content 和本地 NDJSON 的
// message.content 只差几个字符，写错了表现是"界面一直转圈但不长字"，眼睛很难看出来。
//
// 运行：cargo test --lib stream_protocol

use super::{absorb_stream_line, StreamProtocol};

#[test]
fn sse_reads_delta_content() {
    let (mut p, mut c) = (0i64, 0i64);
    let line = r#"data: {"choices":[{"delta":{"content":"她"}}]}"#;
    assert_eq!(
        absorb_stream_line(line, StreamProtocol::Sse, &mut p, &mut c).as_deref(),
        Some("她")
    );
    // 没空格也认（有的网关不加）
    let line2 = r#"data:{"choices":[{"delta":{"content":"走"}}]}"#;
    assert_eq!(
        absorb_stream_line(line2, StreamProtocol::Sse, &mut p, &mut c).as_deref(),
        Some("走")
    );
}

#[test]
fn sse_usage_only_in_last_chunk() {
    let (mut p, mut c) = (0i64, 0i64);
    // 普通内容块不能污染用量
    absorb_stream_line(
        r#"data: {"choices":[{"delta":{"content":"她"}}]}"#,
        StreamProtocol::Sse,
        &mut p,
        &mut c,
    );
    assert_eq!((p, c), (0, 0));
    // 最后一块：choices 空、usage 有值
    let done = r#"data: {"choices":[],"usage":{"prompt_tokens":1234,"completion_tokens":56}}"#;
    assert!(absorb_stream_line(done, StreamProtocol::Sse, &mut p, &mut c).is_none());
    assert_eq!((p, c), (1234, 56));
}

#[test]
fn sse_done_marker_is_ignored() {
    let (mut p, mut c) = (0i64, 0i64);
    assert!(absorb_stream_line("data: [DONE]", StreamProtocol::Sse, &mut p, &mut c).is_none());
}

#[test]
fn ndjson_reads_message_content() {
    let (mut p, mut c) = (0i64, 0i64);
    let line = r#"{"model":"qwen3","message":{"role":"assistant","content":"她"},"done":false}"#;
    assert_eq!(
        absorb_stream_line(line, StreamProtocol::Ndjson, &mut p, &mut c).as_deref(),
        Some("她")
    );
}

#[test]
fn ndjson_done_block_carries_real_usage() {
    let (mut p, mut c) = (0i64, 0i64);
    let line = r#"{"model":"qwen3","message":{"role":"assistant","content":""},"done":true,"done_reason":"stop","prompt_eval_count":1234,"eval_count":56}"#;
    assert!(absorb_stream_line(line, StreamProtocol::Ndjson, &mut p, &mut c).is_none());
    assert_eq!((p, c), (1234, 56));
}

#[test]
fn protocols_do_not_cross_read() {
    // 本地行用 SSE 解析必须什么都拿不到（proto 传错时要"静默无输出"，而不是读出乱七八糟的东西）
    let (mut p, mut c) = (0i64, 0i64);
    assert!(absorb_stream_line(
        r#"{"message":{"content":"她"},"done":false}"#,
        StreamProtocol::Sse,
        &mut p,
        &mut c
    )
    .is_none());
    // 反过来：SSE 行用 NDJSON 解析也读不出增量
    assert!(absorb_stream_line(
        r#"data: {"choices":[{"delta":{"content":"她"}}]}"#,
        StreamProtocol::Ndjson,
        &mut p,
        &mut c
    )
    .is_none());
    assert_eq!((p, c), (0, 0));
}

#[test]
fn blank_and_broken_lines_do_not_break_the_stream() {
    let (mut p, mut c) = (0i64, 0i64);
    for line in ["", "   ", ": ping", "event: message", "data: {不是 JSON}", "{半截"] {
        assert!(absorb_stream_line(line, StreamProtocol::Sse, &mut p, &mut c).is_none());
        assert!(absorb_stream_line(line, StreamProtocol::Ndjson, &mut p, &mut c).is_none());
    }
    assert_eq!((p, c), (0, 0));
}
