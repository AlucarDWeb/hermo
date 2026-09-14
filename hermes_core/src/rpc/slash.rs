//! The slash ladder: the try-then-fallback policy between `slash.exec` and
//! `command.dispatch` (PLAN §4 T10).
//!
//! Adapter layer: pure mapping of wire shapes onto a non-FFI *plan* that the
//! use case (`core.rs`) maps onto its `SlashOutcome` and executes. No socket,
//! no I/O, no FFI type — the policy is unit-testable without either (Clean
//! Architecture: the Dependency Rule keeps `core` above this module, never
//! the reverse).
//!
//! Wire facts (verified against the installed `tui_gateway/methods_tools.py`
//! and `methods_complete.py`, v0.21.1 line):
//! - `slash.exec` with an empty command answers **4004** ("empty command").
//! - A skill command answers **4018** ("skill command: use command.dispatch
//!   for /x") — the client must fall back on 4018, not treat it as a failure.
//! - `command.dispatch {name, arg}` resolves quick > plugin > bundle > skill
//!   > built-in and answers 4018 for anything unknown.
//!
//! Errors are matched **by code**, never by parsing `detail`.

use serde_json::Value;

use super::client::ClientError;

/// How many `Alias` hops `run_slash` may follow before it reports a loop.
/// An alias chain is legitimate (`/a` → `/b` → a real command); a cycle only
/// ever ends at this cap, which is why it is a named constant.
pub const MAX_ALIAS_HOPS: usize = 8;

/// What the caller must do after a `slash.exec` attempt.
#[derive(Debug, Clone, PartialEq)]
pub enum SlashPlan {
    /// `slash.exec` answered successfully: render the output. A `warning`
    /// field, when present, is concatenated Desktop-style
    /// (`warning: …\n{body}` — apps/desktop `slash.ts`).
    Output(String),
    /// 4004 — the gateway refuses an empty command. Not an error: there is
    /// simply nothing to run, so the surface shows nothing and stays open.
    Empty,
    /// 4018 — the command lives behind `command.dispatch {name, arg}`:
    /// `name` is the first token without its leading `/`, `arg` the rest.
    Dispatch { name: String, arg: String },
}

/// Map a `slash.exec` result onto the next step of the ladder.
pub fn plan_after_slash_exec(
    command: &str,
    result: Result<Value, ClientError>,
) -> Result<SlashPlan, ClientError> {
    match result {
        Ok(value) => {
            let body = crate::json::str_at(&value, "output").to_string();
            let warning = crate::json::str_at(&value, "warning");
            Ok(SlashPlan::Output(if warning.is_empty() {
                body
            } else {
                format!("warning: {warning}\n{body}")
            }))
        }
        Err(ClientError::Rpc { code: 4004, .. }) => Ok(SlashPlan::Empty),
        Err(ClientError::Rpc { code: 4018, .. }) => {
            let (name, arg) = split_name_arg(command);
            Ok(SlashPlan::Dispatch { name, arg })
        }
        // Any other code (busy, worker failure, transport) is a real failure
        // and surfaces as-is — we are stricter than Desktop, which falls back
        // to dispatch on ANY throw (PLAN §4 T10: fallback on 4018 only).
        Err(e) => Err(e),
    }
}

/// Split `/name arg…` into the `command.dispatch` params. The leading `/` is
/// optional on the input (the server strips it too); the name keeps no slash.
pub fn split_name_arg(command: &str) -> (String, String) {
    let trimmed = command.trim_start_matches('/');
    match trimmed.split_once(' ') {
        Some((name, arg)) => (name.to_string(), arg.to_string()),
        None => (trimmed.to_string(), String::new()),
    }
}

/// What the caller must do with a `command.dispatch` outcome. `Send` and
/// `Skill` both collapse to [`SlashDispatch::Submit`]: the message goes out
/// through the normal submit path so the user row is published (T7c), the
/// row showing `display` when the outcome carries one.
#[derive(Debug, Clone, PartialEq)]
pub enum SlashDispatch {
    /// `exec`/`plugin` — render the text.
    Output(String),
    /// `prefill` — put the text in the composer input, do not submit.
    Prefill(String),
    /// `send`/`skill` — submit `message`; the user row shows `display` when
    /// non-empty, else the submitted text.
    Submit { message: String, display: String },
    /// `alias` — re-run the whole ladder for `target` (leading `/` preserved
    /// by the caller), up to [`MAX_ALIAS_HOPS`] hops.
    Alias(String),
    /// Unknown outcome `type` — degrade to a short stable output, never a
    /// panic and never a silent `Ok(())` with no surface feedback.
    Unknown,
}

/// Map a parsed [`super::api::DispatchOutcome`] onto the next step.
pub fn plan_from_dispatch(outcome: &super::api::DispatchOutcome) -> SlashDispatch {
    use super::api::DispatchOutcome as D;
    match outcome {
        D::Output(text) => SlashDispatch::Output(text.clone()),
        D::Prefill(text) => SlashDispatch::Prefill(text.clone()),
        D::Send(message) => SlashDispatch::Submit {
            message: message.clone(),
            display: String::new(),
        },
        D::Skill { message, display, .. } => SlashDispatch::Submit {
            message: message.clone(),
            display: display.clone(),
        },
        D::Alias(target) => SlashDispatch::Alias(target.clone()),
        D::Unknown => SlashDispatch::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Every shape below mirrors the verified wire (methods_tools.py,
    // methods_complete.py, v0.21.1) or its documented defensive behaviour.

    #[test]
    fn successful_exec_renders_output_and_concatenates_warning_desktop_style() {
        // Plain output.
        let plan = plan_after_slash_exec("/goal", Ok(json!({"output": "help"}))).expect("plan");
        assert_eq!(plan, SlashPlan::Output("help".into()));
        // A warning rides along (methods_tools.py `payload["warning"] = ...`).
        let plan = plan_after_slash_exec(
            "/goal",
            Ok(json!({"output": "body", "warning": "config drifted"})),
        )
        .expect("plan");
        assert_eq!(plan, SlashPlan::Output("warning: config drifted\nbody".into()));
        // A warning with no output still renders the warning line.
        let plan = plan_after_slash_exec("/goal", Ok(json!({"warning": "w"}))).expect("plan");
        assert_eq!(plan, SlashPlan::Output("warning: w\n".into()));
    }

    #[test]
    fn code_4004_plans_empty_not_an_error() {
        let plan = plan_after_slash_exec(
            "",
            Err(ClientError::Rpc {
                code: 4004,
                detail: "empty command".into(),
            }),
        )
        .expect("4004 is not an error");
        assert_eq!(plan, SlashPlan::Empty);
    }

    #[test]
    fn code_4018_plans_a_dispatch_with_split_name_and_arg() {
        // The skill-command redirect, exactly as the gateway words it.
        let plan = plan_after_slash_exec(
            "/deploy prod",
            Err(ClientError::Rpc {
                code: 4018,
                detail: "skill command: use command.dispatch for /deploy".into(),
            }),
        )
        .expect("plan");
        assert_eq!(
            plan,
            SlashPlan::Dispatch {
                name: "deploy".into(),
                arg: "prod".into()
            }
        );
        // A bare name without an argument dispatches with an empty arg.
        let plan = plan_after_slash_exec(
            "/help",
            Err(ClientError::Rpc { code: 4018, detail: String::new() }),
        )
        .expect("plan");
        assert_eq!(
            plan,
            SlashPlan::Dispatch { name: "help".into(), arg: String::new() }
        );
    }

    #[test]
    fn other_rpc_codes_propagate_as_errors() {
        for code in [4009, 5020, 5030] {
            let result = plan_after_slash_exec(
                "/x",
                Err(ClientError::Rpc { code, detail: "real failure".into() }),
            );
            assert!(
                matches!(result, Err(ClientError::Rpc { code: c, .. }) if c == code),
                "code {code} must propagate, not fall back to dispatch"
            );
        }
    }

    #[test]
    fn split_name_arg_strips_one_leading_slash_and_keeps_the_rest_of_the_arg() {
        assert_eq!(split_name_arg("/model set gpt-5"), ("model".into(), "set gpt-5".into()));
        assert_eq!(split_name_arg("model"), ("model".into(), String::new()));
        assert_eq!(split_name_arg("/snap"), ("snap".into(), String::new()));
        // Only the leading slashes go; the server `lstrip`s them all too.
        assert_eq!(split_name_arg("//weird x"), ("weird".into(), "x".into()));
    }

    #[test]
    fn dispatch_outcomes_map_onto_the_plan_the_ffi_executes() {
        use super::super::api::DispatchOutcome;
        assert_eq!(
            plan_from_dispatch(&DispatchOutcome::Output("out".into())),
            SlashDispatch::Output("out".into())
        );
        assert_eq!(
            plan_from_dispatch(&DispatchOutcome::Prefill("/model ".into())),
            SlashDispatch::Prefill("/model ".into())
        );
        // `send` submits with no display override: the row shows the text.
        assert_eq!(
            plan_from_dispatch(&DispatchOutcome::Send("m".into())),
            SlashDispatch::Submit { message: "m".into(), display: String::new() }
        );
        // `skill` submits the message; a non-empty display wins for the row.
        assert_eq!(
            plan_from_dispatch(&DispatchOutcome::Skill {
                name: "deploy".into(),
                message: "hi".into(),
                display: "/deploy".into(),
            }),
            SlashDispatch::Submit { message: "hi".into(), display: "/deploy".into() }
        );
        assert_eq!(
            plan_from_dispatch(&DispatchOutcome::Alias("/other".into())),
            SlashDispatch::Alias("/other".into())
        );
        // Unknown degrades to the plan's Unknown arm (rendered as a short
        // stable output by the FFI mapping) — never a panic.
        assert_eq!(plan_from_dispatch(&DispatchOutcome::Unknown), SlashDispatch::Unknown);
    }
}
