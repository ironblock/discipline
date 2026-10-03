//! #29 I0's request bodies, rendered by the client's own `wire::streaming_body`. Kept as source beside the
//! captures, not as a crate example: copy it to `diet/examples/` and `cargo build --example i0_bodies` to re-render.
//! Arguments: MODEL NONCE [ASSISTANT_CONTENT USER_RESULT] -- with the last two, turn 2 in the `user` shape.
use diet::client::shape::{Limits, Message, Pin, RequestShape, Role, SamplerCard, SamplerSetting, ToolDefinition};
use diet::client::wire;
use diet::formats::record::json::{self, Value};
use std::collections::BTreeMap;
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let model = args[1].clone();
    let nonce = &args[2];
    let schema = Value::Object(json::line(r#"{"type":"object","properties":{"command":{"type":"string","description":"the command to run in bash"}},"required":["command"]}"#).unwrap());
    let mut kwargs = BTreeMap::new();
    kwargs.insert("enable_thinking".to_owned(), Value::Boolean(false));
    let mut messages = vec![
        Message::new(Role::System, "You are working in a git repository. Use the bash tool to run commands."),
        Message::new(Role::User, format!("[{nonce}] How many files are in the current directory? Run `ls | wc -l` with the bash tool and tell me the number.")),
    ];
    if args.len() > 4 {
        // turn 2, shape `user`: the call as the assistant said it, the output as a user message
        messages.push(Message::new(Role::Assistant, args[3].clone()));
        messages.push(Message::new(Role::User, args[4].clone()));
    }
    let shape = RequestShape {
        model,
        tools: vec![ToolDefinition { name: "bash".to_owned(), schema }],
        messages,
        sampler: SamplerCard::empty()
            .with_decimal(SamplerSetting::Temperature, "0.6").unwrap()
            .with_decimal(SamplerSetting::TopP, "0.95").unwrap()
            .with(SamplerSetting::Seed, Pin::Integer(7)),
        limits: Limits { attempt: Duration::from_secs(600), call: Duration::from_secs(600), max_output_tokens: 512, retries: 0 },
        grammar: None,
        template_kwargs: kwargs,
    };
    print!("{}", wire::streaming_body(&shape));
}
