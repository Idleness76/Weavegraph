use serde::{Deserialize, Serialize};

/// A message in a conversation, containing a role and text content.
///
/// Messages are the primary data structure for representing chat interactions,
/// AI conversations, and communication between nodes in the workflow system.
/// Each message has a role (typically "user", "assistant", or "system") and
/// text content.
///
/// # Examples
///
/// ```
/// use weavegraph::message::Message;
///
/// // Using convenience constructors
/// let user_msg = Message::user("What is the weather?");
/// let assistant_msg = Message::assistant("It's sunny today!");
/// let system_msg = Message::system("You are a helpful assistant.");
///
/// // For custom roles
/// let function_msg = Message::new("function", "Processing complete");
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Message {
    /// The role of the message sender (e.g., "user", "assistant", "system").
    ///
    /// Use the constants on [`Message`] for standardized values.
    pub role: String,
    /// The text content of the message.
    pub content: String,
}

impl Message {
    /// User input message role.
    pub const USER: &'static str = "user";
    /// AI assistant response message role.
    pub const ASSISTANT: &'static str = "assistant";
    /// System prompt or instruction message role.
    pub const SYSTEM: &'static str = "system";

    /// Creates a new message with the specified role and content.
    #[must_use]
    pub fn new(role: &str, content: &str) -> Self {
        Self {
            role: role.to_string(),
            content: content.to_string(),
        }
    }

    /// Creates a user message with the specified content.
    #[must_use]
    pub fn user(content: &str) -> Self {
        Self::new(Self::USER, content)
    }

    /// Creates an assistant message with the specified content.
    #[must_use]
    pub fn assistant(content: &str) -> Self {
        Self::new(Self::ASSISTANT, content)
    }

    /// Creates a system message with the specified content.
    #[must_use]
    pub fn system(content: &str) -> Self {
        Self::new(Self::SYSTEM, content)
    }

    /// Returns true if this message has the specified role.
    #[must_use]
    pub fn has_role(&self, role: &str) -> bool {
        self.role == role
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_construction() {
        let msg = Message {
            role: "user".to_string(),
            content: "hello".to_string(),
        };
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "hello");
    }

    #[test]
    fn test_convenience_constructors() {
        let user_msg = Message::user("Hello");
        assert_eq!(user_msg.role, Message::USER);
        assert_eq!(user_msg.content, "Hello");

        let assistant_msg = Message::assistant("Hi there!");
        assert_eq!(assistant_msg.role, Message::ASSISTANT);
        assert_eq!(assistant_msg.content, "Hi there!");

        let system_msg = Message::system("You are helpful");
        assert_eq!(system_msg.role, Message::SYSTEM);
        assert_eq!(system_msg.content, "You are helpful");

        let custom_msg = Message::new("function", "Result: 42");
        assert_eq!(custom_msg.role, "function");
        assert_eq!(custom_msg.content, "Result: 42");
    }

    #[test]
    fn test_role_checking() {
        let user_msg = Message::user("Hello");
        assert!(user_msg.has_role(Message::USER));
        assert!(!user_msg.has_role(Message::ASSISTANT));
    }

    #[test]
    fn test_serialization() {
        let original = Message::user("Test message");
        let json = serde_json::to_string(&original).expect("Serialization failed");
        let deserialized: Message = serde_json::from_str(&json).expect("Deserialization failed");
        assert_eq!(original, deserialized);
    }
}
