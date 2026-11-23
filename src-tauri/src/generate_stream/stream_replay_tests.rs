// 整段流回放测试（2026-11-23）。
//
// 之前的 `stream_protocol_tests` 只测"**一行**怎么读"；这一组测"**一整条流**怎么拼起来"：
// 把真实形态的 SSE / NDJSON 按**任意字节边界**切开喂进去，断言正文、用量、以及
// "半截 JSON / 流中断 / 尾行没换行"时不会丢内容、不会 panic。
//
// 复用主链路同一份 `LineBuffer` + `absorb_stream_line`（不是另写一个解析器）——
// 这正是"回放测试"的意义：测的是生产代码，不是测试自己的模拟。
//
// 真实上游冒烟（`real_deepseek_stream_smoke`）默认 `#[ignore]`，发版前手动跑：
//   $env:DREAMWEAVER_SMOKE_API_KEY="sk-..." ; cargo test --lib real_deepseek -- --ignored --nocapture

use super::{absorb_stream_line, LineBuffer, StreamProtocol};

/// 按 `splits` 里的字节数切块（循环使用），喂完整条流，返回（正文, prompt_tokens, completion_tokens）。
fn replay(stream: &str, proto: StreamProtocol, splits: &[usize]) -> (String, i64, i64) {
    let bytes = stream.as_bytes();
    let mut lines = LineBuffer::new();
    let mut text = String::new();
    let (mut p, mut c) = (0i64, 0i64);
    let mut i = 0usize;
    let mut k = 0usize;
    while i < bytes.len() {
        let n = splits.get(k).copied().unwrap_or(7).max(1);
        let end = (i + n).min(bytes.len());
        for line in lines.push(&bytes[i..end]) {
            if let Some(d) = absorb_stream_line(&line, proto, &mut p, &mut c) {
                text.push_str(&d);
            }
        }
        i = end;
        k += 1;
    }
    if let Some(line) = lines.flush() {
        if let Some(d) = absorb_stream_line(&line, proto, &mut p, &mut c) {
            text.push_str(&d);
        }
    }
    (text, p, c)
}

/// 拼一条云端形态的 SSE：每块一个 `delta.content`，最后一块带 usage，再一行 `[DONE]`。
fn sse_stream(deltas: &[&str], usage: Option<(i64, i64)>) -> String {
    let mut s = String::new();
    for d in deltas {
        let v = serde_json::json!({"choices": [{"delta": {"content": d}}]});
        s.push_str(&format!("data: {v}\n\n"));
    }
    if let Some((p, c)) = usage {
        let v = serde_json::json!({"choices": [{"delta": {}}], "usage": {"prompt_tokens": p, "completion_tokens": c}});
        s.push_str(&format!("data: {v}\n\n"));
    }
    s.push_str("data: [DONE]\n\n");
    s
}

fn ndjson_stream(deltas: &[&str], usage: Option<(i64, i64)>) -> String {
    let mut s = String::new();
    for d in deltas {
        let v = serde_json::json!({"message": {"content": d}, "done": false});
        s.push_str(&v.to_string());
        s.push('\n');
    }
    let done = match usage {
        Some((p, c)) => serde_json::json!({"message": {"content": ""}, "done": true, "prompt_eval_count": p, "eval_count": c}),
        None => serde_json::json!({"message": {"content": ""}, "done": true}),
    };
    s.push_str(&done.to_string());
    s.push('\n');
    s
}

#[test]
fn sse_replays_same_text_under_any_split() {
    let stream = sse_stream(&["你好", "，世界", "。"], Some((1234, 56)));
    let expect = "你好，世界。";
    let cases: &[&[usize]] = &[&[1], &[2], &[3], &[5], &[7], &[13], &[1, 3, 7, 13, 23]];
    for &splits in cases {
        let (text, p, c) = replay(&stream, StreamProtocol::Sse, &splits);
        assert_eq!(text, expect, "按 {splits:?} 字节切块时正文不对");
        assert_eq!((p, c), (1234, 56), "按 {splits:?} 字节切块时 usage 不对");
    }
}

#[test]
fn sse_split_inside_utf8_char_does_not_lose_text() {
    // 1 字节一喂，必然把中文字符切开；LineBuffer 必须把不完整行放回去等下一块
    let stream = sse_stream(&["夏婉琴", "在门口"], None);
    let (text, _, _) = replay(&stream, StreamProtocol::Sse, &[1]);
    assert_eq!(text, "夏婉琴在门口");
}

#[test]
fn sse_cut_mid_json_keeps_earlier_deltas_and_does_not_panic() {
    let full = sse_stream(&["前半句", "后半句"], None);
    // 从最后一个 data 行中间截断（模拟上游断开）
    let cut = full.find("后半句").unwrap() + 3;
    let broken = &full[..cut];
    let (text, _, _) = replay(broken, StreamProtocol::Sse, &[4]);
    assert_eq!(text, "前半句", "已经收到的增量不能丢；半截行不能 panic");
}

#[test]
fn sse_last_line_without_trailing_newline_is_flushed() {
    let mut stream = sse_stream(&["最后一", "句"], None);
    while stream.ends_with('\n') {
        stream.pop();
    }
    let (text, _, _) = replay(&stream, StreamProtocol::Sse, &[9]);
    assert_eq!(text, "最后一句");
}

#[test]
fn sse_broken_line_is_skipped_but_stream_continues() {
    // 中间夹一行坏 JSON：只丢这一行，前后的增量都要在
    let stream = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"甲\"}}]}\n\n",
        "data: {oops\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"乙\"}}]}\n\n",
        "data: [DONE]\n\n",
    );
    let (text, _, _) = replay(stream, StreamProtocol::Sse, &[5]);
    assert_eq!(text, "甲乙", "坏行只该丢自己，不该拖垮前后的流");
}
#[test]
fn ndjson_replays_with_usage_in_done_block() {
    let stream = ndjson_stream(&["本地", "模型"], Some((321, 12)));
    let cases: &[&[usize]] = &[&[1], &[3], &[11]];
    for &splits in cases {
        let (text, p, c) = replay(&stream, StreamProtocol::Ndjson, &splits);
        assert_eq!(text, "本地模型");
        assert_eq!((p, c), (321, 12));
    }
}

#[test]
fn protocols_do_not_cross_read_in_replay() {
    let sse = sse_stream(&["甲"], None);
    let (t1, _, _) = replay(&sse, StreamProtocol::Ndjson, &[5]);
    assert_eq!(t1, "", "SSE 的行拿 NDJSON 读不出来（也不会 panic）");
    let nd = ndjson_stream(&["乙"], None);
    let (t2, _, _) = replay(&nd, StreamProtocol::Sse, &[5]);
    assert_eq!(t2, "", "NDJSON 的行拿 SSE 读不出来");
}

/// 真实上游冒烟（手动跑，默认不跑）：
///   $env:DREAMWEAVER_SMOKE_API_KEY="sk-..."
///   cargo test --lib real_deepseek -- --ignored --nocapture
///
/// 只验证"真上游的流能被我们的解码器吃下去"：正文非空、用量读得到、不 panic。
/// 没有 Key 时直接打印跳过（CI 里永远不跑，因为没有 `--ignored`）。
#[tokio::test]
#[ignore = "真实上游冒烟：需要 DREAMWEAVER_SMOKE_API_KEY，发版前手动跑"]
async fn real_deepseek_stream_smoke() {
    let key = std::env::var("DREAMWEAVER_SMOKE_API_KEY").unwrap_or_default();
    if key.trim().is_empty() {
        eprintln!("[smoke] 没有 DREAMWEAVER_SMOKE_API_KEY，跳过");
        return;
    }
    let base = std::env::var("DREAMWEAVER_SMOKE_BASE").unwrap_or_else(|_| "https://api.deepseek.com/v1".to_string());
    let model = std::env::var("DREAMWEAVER_SMOKE_MODEL").unwrap_or_else(|_| "deepseek-chat".to_string());
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/chat/completions", base.trim_end_matches('/')))
        .bearer_auth(key)
        .json(&serde_json::json!({
            "model": model,
            "messages": [{"role": "user", "content": "只回复两个字：收到"}],
            "stream": true,
            "max_tokens": 16,
        }))
        .send()
        .await
        .expect("请求发不出去（检查网络/Base/Key）");
    assert!(resp.status().is_success(), "上游返回 {}", resp.status());
    let mut stream = resp.bytes_stream();
    use futures_util::StreamExt;
    let mut lines = LineBuffer::new();
    let mut text = String::new();
    let (mut p, mut c) = (0i64, 0i64);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.expect("读流失败");
        for line in lines.push(&chunk) {
            if let Some(d) = absorb_stream_line(&line, StreamProtocol::Sse, &mut p, &mut c) {
                text.push_str(&d);
            }
        }
    }
    if let Some(line) = lines.flush() {
        if let Some(d) = absorb_stream_line(&line, StreamProtocol::Sse, &mut p, &mut c) {
            text.push_str(&d);
        }
    }
    eprintln!("[smoke] 正文={text:?} usage=({p},{c})");
    assert!(!text.trim().is_empty(), "真上游一个字都没解出来");
}
