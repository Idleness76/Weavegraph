use serde::{Deserialize, Serialize};
use std::fmt;

/// The role of a message sender in a conversation.
///
/// This enum represents the standard roles used in chat-based AI interactions.
/// For custom roles not covered by the standard variants, use [`Role::Custom`].
///
/// # Serialization
///
/// Roles serialize to/from lowercase strings for JSON compatibility:
/// - `Role::User` ↔ `"user"`
/// - `Role::Assistant` ↔ `"assistant"`
/// - `Role::System` ↔ `"system"`
/// - `Role::Tool` ↔ `"tool"`
/// - `Role::Custom("foo")` ↔ `"foo"`
///
/// # Examples
///
/// ```
/// use weavegraph::message::Role;
///
/// let role = Role::User;
/// assert_eq!(role.as_str(), "user");
///
/// let parsed: Role = "assistant".into();
/// assert_eq!(parsed, Role::Assistant);
///
/// // Custom roles for extensibility
/// let custom = Role::Custom("function".to_string());
/// assert_eq!(custom.as_str(), "function");
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum Role {
    /// User input message role.
    #[default]
    User,
    /// AI assistant response message role.
    Assistant,
    /// System prompt or instruction message role.
    System,
    /// Tool/function call result message role.
    Tool,
    /// Custom role for extensibility (e.g., "function", "context").
    Custom(String),
}

impl Role {
    /// Returns the string representation of this role.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::System => "system",
            Role::Tool => "tool",
            Role::Custom(s) => s.as_str(),
        }
    }

    /// Returns true if this role matches the given string.
    #[must_use]
    pub fn matches(&self, role_str: &str) -> bool {
        self.as_str() == role_str
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl From<&str> for Role {
    fn from(s: &str) -> Self {
        match s {
            "user" => Role::User,
            "assistant" => Role::Assistant,
            "system" => Role::System,
            "tool" => Role::Tool,
            other => Role::Custom(other.to_string()),
        }
    }
}

impl From<String> for Role {
    fn from(s: String) -> Self {
        Role::from(s.as_str())
    }
}

impl From<Role> for String {
    fn from(role: Role) -> Self {
        role.as_str().to_string()
    }
}

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
/// use weavegraph::message::{Message, Role};
///
/// // Using convenience constructors (recommended)
/// let user_msg = Message::user("What is the weather?");
/// let assistant_msg = Message::assistant("It's sunny today!");
/// let system_msg = Message::system("You are a helpful assistant.");
///
/// // Using Role enum directly
/// let msg = Message::with_role(Role::User, "Hello!");
/// assert!(msg.is_role(Role::User));
///
/// // For custom roles
/// let function_msg = Message::with_role(Role::Custom("function".into()), "Result: 42");
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Message {
    /// The role of the message sender.
    ///
    /// This field is serialized as a string for backward compatibility.
    /// Use [`role_type()`](Self::role_type) to get the typed [`Role`] enum,
    /// or [`is_role()`](Self::is_role) for type-safe role checking.
    #[serde(deserialize_with = "deserialize_role_as_string")]
    pub role: String,
    /// The text content of the message.
    pub content: String,
}

/// Custom deserializer that accepts both Role enum and plain strings.
fn deserialize_role_as_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Ok(s)
}

impl Message {
    /// User input message role.
    #[deprecated(since = "0.2.0", note = "Use Role::User instead")]
    pub const USER: &'static str = "user";
    /// AI assistant response message role.
    #[deprecated(since = "0.2.0", note = "Use Role::Assistant instead")]
    pub const ASSISTANT: &'static str = "assistant";
    /// System prompt or instruction message role.
    #[deprecated(since = "0.2.0", note = "Use Role::System instead")]
    pub const SYSTEM: &'static str = "system";

    /// Creates a new message with the specified role string and content.
    ///
    /// For type-safe role handling, prefer [`with_role()`](Self::with_role).
    #[must_use]
    pub fn new(role: &str, content: &str) -> Self {
        Self {
            role: role.to_string(),
            content: content.to_string(),
        }
    }

    /// Creates a new message with a typed [`Role`] and content.
    ///
    /// This is the recommended way to create messages with standard roles.
    ///
    /// # Examples
    ///
    /// ```
    /// use weavegraph::message::{Message, Role};
    ///
    /// let msg = Message::with_role(Role::Assistant, "Hello!");
    /// assert!(msg.is_role(Role::Assistant));
    /// ```
    #[must_use]
    pub fn with_role(role: Role, content: &str) -> Self {
        Self {
            role: role.as_str().to_string(),
            content: content.to_string(),
        }
    }

    /// Creates a user message with the specified content.
    #[must_use]
    pub fn user(content: &str) -> Self {
        Self::with_role(Role::User, content)
    }

    /// Creates an assistant message with the specified content.
    #[must_use]
    pub fn assistant(content: &str) -> Self {
        Self::with_role(Role::Assistant, content)
    }

    /// Creates a system message with the specified content.
    #[must_use]
    pub fn system(content: &str) -> Self {
        Self::with_role(Role::System, content)
    }

    /// Creates a tool message with the specified content.
    #[must_use]
    pub fn tool(content: &str) -> Self {
        Self::with_role(Role::Tool, content)
    }

    /// Returns the typed [`Role`] for this message.
    ///
    /// This parses the internal role string into the appropriate [`Role`] variant.
    ///
    /// # Examples
    ///
    /// ```
    /// use weavegraph::message::{Message, Role};
    ///
    /// let msg = Message::user("Hello");
    /// assert_eq!(msg.role_type(), Role::User);
    ///
    /// let custom = Message::new("function", "Result");
    /// assert_eq!(custom.role_type(), Role::Custom("function".into()));
    /// ```
    #[must_use]
    pub fn role_type(&self) -> Role {
        Role::from(self.role.as_str())
    }

    /// Returns true if this message has the specified [`Role`].
    ///
    /// This is the type-safe way to check message roles.
    ///
    /// # Examples
    ///
    /// ```
    /// use weavegraph::message::{Message, Role};
    ///
    /// let msg = Message::user("Hello");
    /// assert!(msg.is_role(Role::User));
    /// assert!(!msg.is_role(Role::Assistant));
    /// ```
    #[must_use]
    pub fn is_role(&self, role: Role) -> bool {
        self.role == role.as_str()
    }

    /// Returns true if this message has the specified role string.
    ///
    /// For type-safe role checking, prefer [`is_role()`](Self::is_role).
    #[must_use]
    #[deprecated(since = "0.2.0", note = "Use is_role(Role::...) instead")]
    pub fn has_role(&self, role: &str) -> bool {
        self.role == role
    }
}

#[cfg(feature = "llm")]
impl From<Message> for rig::completion::Message {
    fn from(msg: Message) -> Self {
        match msg.role_type() {
            Role::User => rig::completion::Message::user(msg.content),
            Role::Assistant => rig::completion::Message::assistant(msg.content),
            // rig doesn't have a system message type - it's typically handled
            // via preamble/system prompt on the completion request itself.
            // We'll treat it as a user message for compatibility.
            Role::System => rig::completion::Message::user(msg.content),
            Role::Tool => rig::completion::Message::user(msg.content),
            // For any custom roles, default to user message
            Role::Custom(_) => rig::completion::Message::user(msg.content),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_from_str() {
        assert_eq!(Role::from("user"), Role::User);
        assert_eq!(Role::from("assistant"), Role::Assistant);
        assert_eq!(Role::from("system"), Role::System);
        assert_eq!(Role::from("tool"), Role::Tool);
        assert_eq!(Role::from("custom"), Role::Custom("custom".to_string()));
    }

    #[test]
    fn test_role_as_str() {
        assert_eq!(Role::User.as_str(), "user");
        assert_eq!(Role::Assistant.as_str(), "assistant");
        assert_eq!(Role::System.as_str(), "system");
        assert_eq!(Role::Tool.as_str(), "tool");
        assert_eq!(Role::Custom("foo".into()).as_str(), "foo");
    }

    #[test]
    fn test_message_role_type() {
        let msg = Message::user("hello");
        assert_eq!(msg.role_type(), Role::User);

        let msg = Message::assistant("hi");
        assert_eq!(msg.role_type(), Role::Assistant);

        let msg = Message::new("custom", "data");
        assert_eq!(msg.role_type(), Role::Custom("custom".into()));
    }

    #[test]
    fn test_message_is_role() {
        let msg = Message::user("hello");
        assert!(msg.is_role(Role::User));
        assert!(!msg.is_role(Role::Assistant));
    }

    #[test]
    fn test_message_with_role() {
        let msg = Message::with_role(Role::Tool, "result");
        assert_eq!(msg.role, "tool");
        assert_eq!(msg.content, "result");
    }

    #[test]
    fn test_role_serialization() {
        let role = Role::User;
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, "\"user\"");

        let parsed: Role = serde_json::from_str("\"assistant\"").unwrap();
        assert_eq!(parsed, Role::Assistant);

        let custom: Role = serde_json::from_str("\"function\"").unwrap();
        assert_eq!(custom, Role::Custom("function".into()));
    }

    #[test]
    fn test_message_backward_compatibility() {
        // Old-style JSON should still parse
        let json = r#"{"role": "user", "content": "hello"}"#;
        let msg: Message = serde_json::from_str(json).unwrap();
        assert_eq!(msg.role, "user");
        assert!(msg.is_role(Role::User));
    }
}
