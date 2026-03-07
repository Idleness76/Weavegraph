use crate::message::{Message, Role};
use rig::completion::message::{
    AssistantContent, Message as RigMessage, ToolResultContent, UserContent,
};

impl From<Message> for RigMessage {
    fn from(msg: Message) -> Self {
        match msg.role {
            Role::User => RigMessage::user(msg.content),
            Role::Assistant => RigMessage::assistant(msg.content),
            // Rig's core completion history is user/assistant-focused; map
            // non-native roles to user for compatibility.
            Role::System | Role::Tool | Role::Custom(_) => RigMessage::user(msg.content),
        }
    }
}

impl From<RigMessage> for Message {
    fn from(msg: RigMessage) -> Self {
        match msg {
            RigMessage::User { content } => Message::with_role(
                Role::User,
                &content
                    .iter()
                    .find_map(extract_user_content_text)
                    .unwrap_or_default(),
            ),
            RigMessage::Assistant { content, .. } => Message::with_role(
                Role::Assistant,
                &content
                    .iter()
                    .find_map(extract_assistant_content_text)
                    .unwrap_or_default(),
            ),
        }
    }
}

fn extract_user_content_text(content: &UserContent) -> Option<String> {
    match content {
        UserContent::Text(text) => Some(text.text.clone()),
        UserContent::ToolResult(result) => result.content.iter().find_map(|chunk| match chunk {
            ToolResultContent::Text(text) => Some(text.text.clone()),
            ToolResultContent::Image(_) => None,
        }),
        UserContent::Image(_)
        | UserContent::Audio(_)
        | UserContent::Video(_)
        | UserContent::Document(_) => None,
    }
}

fn extract_assistant_content_text(content: &AssistantContent) -> Option<String> {
    match content {
        AssistantContent::Text(text) => Some(text.text.clone()),
        AssistantContent::Reasoning(reasoning) => reasoning.reasoning.first().cloned(),
        AssistantContent::ToolCall(call) => Some(format!("[tool_call:{}]", call.function.name)),
        AssistantContent::Image(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first_user_text(msg: RigMessage) -> Option<String> {
        match msg {
            RigMessage::User { content } => content.iter().find_map(extract_user_content_text),
            RigMessage::Assistant { .. } => None,
        }
    }

    #[test]
    fn maps_weavegraph_roles_to_rig_messages() {
        let user = RigMessage::from(Message::user("u"));
        let assistant = RigMessage::from(Message::assistant("a"));
        let system = RigMessage::from(Message::system("s"));
        let tool = RigMessage::from(Message::tool("t"));
        let custom = RigMessage::from(Message::with_role(Role::Custom("worker".into()), "c"));

        assert_eq!(first_user_text(user), Some("u".to_string()));
        assert_eq!(first_user_text(system), Some("s".to_string()));
        assert_eq!(first_user_text(tool), Some("t".to_string()));
        assert_eq!(first_user_text(custom), Some("c".to_string()));

        match assistant {
            RigMessage::Assistant { content, .. } => {
                assert_eq!(
                    content.iter().find_map(extract_assistant_content_text),
                    Some("a".to_string())
                );
            }
            RigMessage::User { .. } => panic!("assistant role should map to rig assistant"),
        }
    }

    #[test]
    fn maps_rig_messages_to_weavegraph_messages() {
        let user: Message = RigMessage::user("hello").into();
        assert_eq!(user.role, Role::User);
        assert_eq!(user.content, "hello");

        let assistant: Message = RigMessage::assistant("world").into();
        assert_eq!(assistant.role, Role::Assistant);
        assert_eq!(assistant.content, "world");
    }

    #[test]
    fn preserves_text_from_rig_tool_result_user_messages() {
        let tool_result: Message = RigMessage::tool_result("tool-1", "ok").into();
        assert_eq!(tool_result.role, Role::User);
        assert_eq!(tool_result.content, "ok");
    }
}
