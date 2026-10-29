//! Tool calling for models that have none (many local models): the model replies with one
//! fenced JSON action per turn, and SayQL answers with the tool's result as a plain message.

use super::model::{Block, Msg, Role, ToolSpec, Turn, render};
use serde_json::Value;

/// What the model is told about the protocol and the tools.
pub fn instructions(tools: &[ToolSpec]) -> String {
    let list: Vec<String> = tools
        .iter()
        .map(|t| format!("- {}: {}\n  input: {}", t.name, t.description, t.schema))
        .collect();
    format!(
        "Tools. You can't call tools directly. Instead, end every reply with exactly one action, a fenced JSON block:\n\
         ```json\n{{\"tool\": \"<name>\", \"input\": {{ ... }}}}\n```\n\
         SayQL runs it and replies with the result. One action per reply; always end with `finish`.\n\n{}",
        list.join("\n")
    )
}

/// The conversation as plain text messages, for a model without tool calling.
pub fn to_text(messages: &[Msg]) -> Vec<Msg> {
    messages
        .iter()
        .map(|m| {
            let text = match m.role {
                Role::User
                    if m.blocks
                        .iter()
                        .any(|b| matches!(b, Block::ToolResult { .. })) =>
                {
                    format!("Result:\n{}", render(m))
                }
                Role::Assistant => m
                    .blocks
                    .iter()
                    .map(|b| match b {
                        Block::Text(t) => t.clone(),
                        Block::ToolUse { name, input, .. } => {
                            format!(
                                "```json\n{}\n```",
                                serde_json::json!({ "tool": name, "input": input })
                            )
                        }
                        _ => String::new(),
                    })
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n"),
                Role::User => render(m),
            };
            Msg {
                role: m.role,
                blocks: vec![Block::Text(text)],
            }
        })
        .collect()
}

/// The model's reply, with its last fenced JSON action read as a tool call.
pub fn from_text(mut turn: Turn) -> Turn {
    let text = turn.text();
    let Some((before, action)) = last_action(&text) else {
        return turn;
    };
    let mut blocks = Vec::new();
    if !before.trim().is_empty() {
        blocks.push(Block::Text(before.trim().to_string()));
    }
    let name = action["tool"].as_str().unwrap_or_default().to_string();
    blocks.push(Block::ToolUse {
        id: format!("text-{}", uuid::Uuid::new_v4()),
        name,
        input: action["input"].clone(),
    });
    turn.blocks = blocks;
    turn
}

/// The last ```json block (or bare JSON object) that names a tool, and the text before it.
fn last_action(text: &str) -> Option<(&str, Value)> {
    let mut found = None;
    let mut from = 0;
    while let Some(i) = text[from..].find("```") {
        let start = from + i;
        let body_start = text[start + 3..].find('\n').map(|n| start + 3 + n + 1)?;
        let Some(len) = text[body_start..].find("```") else {
            break;
        };
        let body = &text[body_start..body_start + len];
        if let Ok(v) = serde_json::from_str::<Value>(body.trim())
            && v["tool"].is_string()
        {
            found = Some((&text[..start], v));
            from = body_start + len + 3;
        } else {
            // Not an action: maybe a stray fence, so the next ``` may open the real block.
            from = start + 3;
        }
    }
    if found.is_none() {
        let start = text.find('{')?;
        let v: Value = serde_json::from_str(text[start..].trim()).ok()?;
        if v["tool"].is_string() {
            found = Some((&text[..start], v));
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn reply(text: &str) -> Turn {
        Turn {
            blocks: vec![Block::Text(text.into())],
            ..Default::default()
        }
    }

    #[test]
    fn reads_the_fenced_action() {
        let t = from_text(reply(
            "Let me look.\n```json\n{\"tool\": \"run_query\", \"input\": {\"sql\": \"SELECT 1\"}}\n```",
        ));
        assert_eq!(t.blocks[0], Block::Text("Let me look.".into()));
        let (_, name, input) = t.tool_uses().next().unwrap();
        assert_eq!((name, input), ("run_query", &json!({ "sql": "SELECT 1" })));
        let bare = from_text(reply(
            "{\"tool\": \"finish\", \"input\": {\"answer\": \"x\"}}",
        ));
        assert_eq!(bare.tool_uses().next().unwrap().1, "finish");
        assert!(
            from_text(reply("No action here."))
                .tool_uses()
                .next()
                .is_none()
        );
        // A stray fence before the action (seen from Claude through Claude Code).
        let stray = from_text(reply(
            "<invoke name=\"run_query\">\n</invoke>\n```\n\nActually, the required format.\n\n```json\n{\"tool\": \"run_query\", \"input\": {\"sql\": \"SELECT 2\"}}\n```",
        ));
        assert_eq!(
            stray.tool_uses().next().unwrap().2,
            &json!({ "sql": "SELECT 2" })
        );
        assert!(
            from_text(reply("```sql\nSELECT 1\n```"))
                .tool_uses()
                .next()
                .is_none()
        );
    }

    #[test]
    fn writes_results_as_text() {
        let msgs = vec![
            Msg {
                role: Role::Assistant,
                blocks: vec![Block::ToolUse {
                    id: "a".into(),
                    name: "run_query".into(),
                    input: json!({}),
                }],
            },
            Msg {
                role: Role::User,
                blocks: vec![Block::ToolResult {
                    id: "a".into(),
                    content: "3 rows".into(),
                    is_error: false,
                }],
            },
        ];
        let t = to_text(&msgs);
        assert!(matches!(&t[0].blocks[0], Block::Text(s) if s.contains("\"tool\":\"run_query\"")));
        assert!(matches!(&t[1].blocks[0], Block::Text(s) if s == "Result:\n3 rows"));
    }
}
