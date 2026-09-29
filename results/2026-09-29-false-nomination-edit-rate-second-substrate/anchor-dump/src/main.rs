//! JSONL wrapper over the gym's literal tier, for the nomination instrument's grader.
//!   anchor-dump anchors   {"text": ...}                 -> {"anchors": [{"text": ...}]}
//!   anchor-dump find      {"needle": ..., "haystack": ...} -> {"hits": [offset, ...]}
use diet::capture::collector::literal;
use std::io::{BufRead, Write};

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.expect("stdin");
        if line.trim().is_empty() { continue; }
        let v: serde_json::Value = serde_json::from_str(&line).expect("a JSON line");
        let row = match mode.as_str() {
            "anchors" => {
                let a = literal::anchors(v["text"].as_str().unwrap_or(""));
                serde_json::json!({"anchors": a.iter().map(|x| serde_json::json!({"text": x.text})).collect::<Vec<_>>()})
            }
            "find" => {
                let h = literal::find(v["needle"].as_str().unwrap_or(""), v["haystack"].as_str().unwrap_or(""));
                serde_json::json!({"hits": h.iter().map(|x| x.offset).collect::<Vec<_>>()})
            }
            m => { eprintln!("anchor-dump: unknown mode {m:?}"); std::process::exit(2) }
        };
        writeln!(out, "{row}").expect("stdout");
    }
}
