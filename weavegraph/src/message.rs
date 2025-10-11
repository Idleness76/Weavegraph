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
