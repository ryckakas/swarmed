//! Asking an ACP agent which models it offers, so nobody has to type them in.
//!
//! The agent is started, greeted, and asked to open a session, which is when it says what it
//! can run on. Nothing is sent to a model, and the agent is stopped as soon as it has answered.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{ChildStdin, ChildStdout, Command};

/// How long the agent may take to say what it offers.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(45);

/// The models `program` offers when started with `args`, as it names them. An `Err` says, in
/// words for the user, why they could not be had.
pub(crate) async fn offered(program: &Path, args: &[String]) -> Result<Vec<String>, String> {
    let asking = ask(program, args);
    let answer = tokio::time::timeout(ANSWER_TIMEOUT, asking).await;
    let opened = answer.map_err(|_| "it did not answer in time".to_string())??;
    let models = models_in(&opened);
    if models.is_empty() {
        return Err("it did not say which models it offers".to_string());
    }
    Ok(models)
}

/// Starts the agent and returns what it answers to opening a session.
async fn ask(program: &Path, args: &[String]) -> Result<Value, String> {
    let cwd = std::env::temp_dir();
    let mut child = Command::new(program)
        .args(args)
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("it could not be started: {error}"))?;
    let pipes = child.stdin.take().zip(child.stdout.take());
    let (mut stdin, stdout) = pipes.ok_or("its pipes are missing")?;
    let mut lines = BufReader::new(stdout).lines();

    let greeting = json!({
        "protocolVersion": 1,
        "clientCapabilities": { "fs": { "readTextFile": false, "writeTextFile": false } },
        "clientInfo": { "name": "swarmed", "version": env!("CARGO_PKG_VERSION") },
    });
    call(&mut stdin, &mut lines, (1, "initialize"), &greeting).await?;
    let session = json!({ "cwd": cwd, "mcpServers": [] });
    call(&mut stdin, &mut lines, (2, "session/new"), &session).await
}

/// One request to the agent, and the result of its answer. What it says in between, to
/// nobody in particular, is passed over.
async fn call(
    stdin: &mut ChildStdin,
    lines: &mut Lines<BufReader<ChildStdout>>,
    (id, method): (u64, &str),
    params: &Value,
) -> Result<Value, String> {
    let request = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
    let gone = |_| "it stopped before answering".to_string();
    stdin
        .write_all(format!("{request}\n").as_bytes())
        .await
        .map_err(gone)?;
    while let Some(line) = lines.next_line().await.map_err(gone)? {
        let message: Value = serde_json::from_str(&line).unwrap_or_default();
        if message.get("id").and_then(Value::as_u64) == Some(id) {
            return answer_of(&message);
        }
    }
    Err("it stopped before answering".to_string())
}

/// The result in an answer, or the agent's own words for why there is none.
fn answer_of(message: &Value) -> Result<Value, String> {
    if let Some(result) = message.get("result") {
        return Ok(result.clone());
    }
    let reason = message.pointer("/error/message").and_then(Value::as_str);
    Err(reason.unwrap_or("it refused").to_string())
}

/// The models named in what an agent answers to opening a session: the values of its `model`
/// option, or, for agents that have no such option, its list of available models.
fn models_in(opened: &Value) -> Vec<String> {
    let named = |list: Option<&Value>, key: &str| -> Vec<String> {
        let items = list.and_then(Value::as_array);
        let name = |item: &Value| item.get(key)?.as_str().map(String::from);
        items.map_or_else(Vec::new, |items| items.iter().filter_map(name).collect())
    };
    let options = opened.get("configOptions").and_then(Value::as_array);
    let is_model = |option: &&Value| option.get("id").and_then(Value::as_str) == Some("model");
    let model = options.and_then(|options| options.iter().find(is_model));
    let chosen = named(model.and_then(|model| model.get("options")), "value");
    if !chosen.is_empty() {
        return chosen;
    }
    named(opened.pointer("/models/availableModels"), "modelId")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn models_are_read_from_the_model_option_or_else_from_the_list() {
        let both = json!({
            "models": { "availableModels": [{ "modelId": "a[low]" }, { "modelId": "a[high]" }] },
            "configOptions": [
                { "id": "mode", "options": [{ "value": "agent" }] },
                { "id": "model", "options": [{ "value": "a", "name": "A" }, { "value": "b" }] },
            ],
        });
        assert_eq!(models_in(&both), ["a", "b"]);
        let listed = json!({ "models": { "availableModels": [{ "modelId": "x" }] } });
        assert_eq!(models_in(&listed), ["x"]);
        assert_eq!(
            models_in(&json!({ "sessionId": "s" })),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_refusal_is_given_in_the_agents_own_words() {
        let refused = json!({ "id": 2, "error": { "code": -32000, "message": "sign in first" } });
        assert_eq!(answer_of(&refused), Err("sign in first".to_string()));
        assert_eq!(
            answer_of(&json!({ "id": 2 })),
            Err("it refused".to_string())
        );
        assert_eq!(answer_of(&json!({ "id": 1, "result": {} })), Ok(json!({})));
    }
}
