//! Whether a served chat template can render a role where the interview
//! asks it (#599).
//!
//! The question is put to the template itself: it is rendered, as the
//! server would render it, over a probe of the shape a fork sends -- the
//! system message, a turn, the assistant's answer, then the interview's ask
//! in the role under test -- and a template that refuses (its own
//! `raise_exception`, such as the stock Qwen3.8 template's "System message
//! must be at the beginning." or "Unexpected message role.") is quoted back.
//! No list of which templates allow which roles is kept here.

use minijinja::{Environment, Error, ErrorKind, Value, context};

/// The probe's ask, in `role`, rendered through `template`: `Ok` when the
/// template renders it, or the template's refusal (or why it could not be
/// rendered at all) as a sentence.
///
/// # Errors
///
/// The template refuses the role, or cannot be rendered over even a plain
/// user ask, in which case no other role can be confirmed either.
pub fn renders(template: &str, role: &str) -> Result<(), String> {
    let probe = |last: &str| {
        render(
            template,
            &[
                ("system", "the standing instruction"),
                ("user", "the turn"),
                ("assistant", "the answer"),
                (last, "the interview's question"),
            ],
        )
    };
    probe("user").map_err(|why| {
        format!("the served template cannot be rendered over a plain user ask, so no role can be confirmed: {why}")
    })?;
    probe(role)
        .map(|_| ())
        .map_err(|why| format!("the served template refuses a `{role}` ask: {why}"))
}

/// `messages` rendered through `template`, with a generation prompt.
fn render(template: &str, messages: &[(&str, &str)]) -> Result<String, String> {
    let mut env = Environment::new();
    minijinja_contrib::add_to_environment(&mut env);
    env.set_unknown_method_callback(minijinja_contrib::pycompat::unknown_method_callback);
    env.add_function(
        "raise_exception",
        |message: String| -> Result<Value, Error> {
            Err(Error::new(ErrorKind::InvalidOperation, message))
        },
    );
    env.add_template("chat", template)
        .map_err(|why| why.to_string())?;
    let messages: Vec<Value> = messages
        .iter()
        .map(|(role, content)| context! { role => *role, content => *content })
        .collect();
    env.get_template("chat")
        .and_then(|chat| {
            chat.render(context! {
                messages => messages,
                add_generation_prompt => true,
                bos_token => "",
                eos_token => "",
            })
        })
        .map_err(|why| {
            // The template's own words when it raised, not the engine's frame.
            if why.kind() == ErrorKind::InvalidOperation {
                why.detail().map_or_else(|| why.to_string(), str::to_owned)
            } else {
                why.to_string()
            }
        })
}

#[cfg(test)]
mod tests {
    use super::renders;

    /// The stock Qwen3.8 template, as the 3.8 line's admission captured it
    /// from the server's `/v1/model` (`prompt_template_content`).
    fn qwen38() -> String {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../substrates/admission/accel24-tabbyapi-exl3-qwen38-27b-3p00/6a96d5696231/raw/model.json"
        );
        let card: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).expect("the capture"))
                .expect("JSON");
        card["parameters"]["prompt_template_content"]
            .as_str()
            .expect("the template")
            .to_owned()
    }

    /// #599: on the floor's template only `user` renders; a `system` ask is
    /// refused in the template's own words, and so is `developer`.
    #[test]
    fn the_stock_qwen38_template_renders_only_a_user_ask() {
        let template = qwen38();
        assert_eq!(renders(&template, "user"), Ok(()));
        let system = renders(&template, "system").expect_err("refused");
        assert!(
            system.contains("System message must be at the beginning."),
            "{system}"
        );
        let developer = renders(&template, "developer").expect_err("refused");
        assert!(
            developer.contains("Unexpected message role."),
            "{developer}"
        );
    }

    /// A template that renders any role renders the ask in it; one the
    /// engine cannot render at all confirms nothing.
    #[test]
    fn a_permissive_template_renders_every_role_and_a_broken_one_none() {
        let permissive = "{% for m in messages %}<{{ m.role }}>{{ m.content }}{% endfor %}";
        for role in ["user", "system", "developer"] {
            assert_eq!(renders(permissive, role), Ok(()), "{role}");
        }
        let broken = "{% for m in messages %}{{ m.content | no_such_filter }}{% endfor %}";
        let why = renders(broken, "system").expect_err("nothing confirmed");
        assert!(why.contains("plain user ask"), "{why}");
    }
}
