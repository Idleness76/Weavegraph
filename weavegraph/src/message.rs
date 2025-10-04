use serde::{Deserialize, Serialize};

/// Common role constants for message types in chat/AI interactions.
///
/// These constants provide type-safe, standardized role values for creating messages,
/// reducing typos and ensuring consistency across the codebase.
pub mod roles {
    /// User input message role.
    pub const USER: &str = "user";
    /// AI assistant response message role.
    pub const ASSISTANT: &str = "assistant";
    /// System prompt or instruction message role.
    pub const SYSTEM: &str = "system";
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
/// ## Basic Construction
/// ```
/// use weavegraph::message::{Message, roles};
///
/// // Manual construction
/// let message = Message {
///     role: roles::USER.to_string(),
///     content: "Hello, world!".to_string(),
/// };
///
/// // Using convenience constructors
/// let user_msg = Message::user("What is the weather?");
/// let assistant_msg = Message::assistant("It's sunny today!");
/// let system_msg = Message::system("You are a helpful assistant.");
/// ```
///
/// ## Builder Pattern
/// ```
/// use weavegraph::message::Message;
///
/// let message = Message::builder()
///     .role("user")
///     .content("Tell me a joke")
///     .build();
/// ```
///
/// # Serialization
///
/// Messages implement `Serialize` and `Deserialize` for JSON/other formats:
/// ```
/// use weavegraph::message::Message;
///
/// let msg = Message::user("test");
/// let json = serde_json::to_string(&msg).unwrap();
/// let parsed: Message = serde_json::from_str(&json).unwrap();
/// assert_eq!(msg, parsed);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Message {
    /// The role of the message sender (e.g., "user", "assistant", "system").
    ///
    /// Use the constants in [`roles`] module for standardized values.
    pub role: String,
    /// The text content of the message.
    pub content: String,
}

impl Message {
    /// Creates a new message with the specified role and content.
    ///
    /// # Examples
    /// ```
    /// use weavegraph::message::{Message, roles};
    ///
    /// let msg = Message::new(roles::USER, "Hello!");
    /// assert_eq!(msg.role, "user");
    /// assert_eq!(msg.content, "Hello!");
    /// ```
    #[must_use]
    pub fn new(role: &str, content: &str) -> Self {
        Self {
            role: role.to_string(),
            content: content.to_string(),
        }
    }

    /// Creates a user message with the specified content.
    ///
    /// # Examples
    /// ```
    /// use weavegraph::message::Message;
    ///
    /// let msg = Message::user("What's the weather like?");
    /// assert_eq!(msg.role, "user");
    /// assert_eq!(msg.content, "What's the weather like?");
    /// ```
    #[must_use]
    pub fn user(content: &str) -> Self {
        Self::new(roles::USER, content)
    }

    /// Creates an assistant message with the specified content.
    ///
    /// # Examples
    /// ```
    /// use weavegraph::message::Message;
    ///
    /// let msg = Message::assistant("It's sunny and 75°F.");
    /// assert_eq!(msg.role, "assistant");
    /// assert_eq!(msg.content, "It's sunny and 75°F.");
    /// ```
    #[must_use]
    pub fn assistant(content: &str) -> Self {
        Self::new(roles::ASSISTANT, content)
    }

    /// Creates a system message with the specified content.
    ///
    /// # Examples
    /// ```
    /// use weavegraph::message::Message;
    ///
    /// let msg = Message::system("You are a helpful AI assistant.");
    /// assert_eq!(msg.role, "system");
    /// assert_eq!(msg.content, "You are a helpful AI assistant.");
    /// ```
    #[must_use]
    pub fn system(content: &str) -> Self {
        Self::new(roles::SYSTEM, content)
    }

    /// Creates a message builder for fluent construction.
    ///
    /// # Examples
    /// ```
    /// use weavegraph::message::Message;
    ///
    /// let msg = Message::builder()
    ///     .role("function")
    ///     .content("Result: 42")
    ///     .build();
    /// ```
    #[must_use]
    pub fn builder() -> MessageBuilder {
        MessageBuilder::new()
    }

    /// Validates that the message has non-empty role and content.
    ///
    /// # Examples
    /// ```
    /// use weavegraph::message::Message;
    ///
    /// let valid = Message::user("Hello");
    /// assert!(valid.is_valid());
    ///
    /// let invalid = Message::new("", "");
    /// assert!(!invalid.is_valid());
    /// ```
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self.role.trim().is_empty() && !self.content.trim().is_empty()
    }

    /// Returns true if this message has the specified role.
    ///
    /// # Examples
    /// ```
    /// use weavegraph::message::{Message, roles};
    ///
    /// let msg = Message::user("Hello");
    /// assert!(msg.has_role(roles::USER));
    /// assert!(!msg.has_role(roles::ASSISTANT));
    /// ```
    #[must_use]
    pub fn has_role(&self, role: &str) -> bool {
        self.role == role
    }

    /// Returns true if this message is from a user.
    ///
    /// # Examples
    /// ```
    /// use weavegraph::message::Message;
    ///
    /// let user_msg = Message::user("Hello");
    /// let assistant_msg = Message::assistant("Hi there!");
    ///
    /// assert!(user_msg.is_user());
    /// assert!(!assistant_msg.is_user());
    /// ```
    #[must_use]
    pub fn is_user(&self) -> bool {
        self.has_role(roles::USER)
    }

    /// Returns true if this message is from an assistant.
    ///
    /// # Examples
    /// ```
    /// use weavegraph::message::Message;
    ///
    /// let user_msg = Message::user("Hello");
    /// let assistant_msg = Message::assistant("Hi there!");
    ///
    /// assert!(!user_msg.is_assistant());
    /// assert!(assistant_msg.is_assistant());
    /// ```
    #[must_use]
    pub fn is_assistant(&self) -> bool {
        self.has_role(roles::ASSISTANT)
    }

    /// Returns true if this message is a system message.
    ///
    /// # Examples
    /// ```
    /// use weavegraph::message::Message;
    ///
    /// let system_msg = Message::system("You are helpful");
    /// let user_msg = Message::user("Hello");
    ///
    /// assert!(system_msg.is_system());
    /// assert!(!user_msg.is_system());
    /// ```
    #[must_use]
    pub fn is_system(&self) -> bool {
        self.has_role(roles::SYSTEM)
    }
}

/// Builder for constructing `Message` instances with a fluent API.
///
/// Provides a more flexible way to construct messages when you need
/// to conditionally set fields or when the message construction is complex.
///
/// # Examples
/// ```
/// use weavegraph::message::Message;
///
/// let msg = Message::builder()
///     .role("function")
///     .content("Processing complete")
///     .build();
/// ```
#[derive(Default)]
pub struct MessageBuilder {
    role: Option<String>,
    content: Option<String>,
}

impl MessageBuilder {
    /// Creates a new message builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the role for the message.
    ///
    /// # Examples
    /// ```
    /// use weavegraph::message::{Message, roles};
    ///
    /// let msg = Message::builder()
    ///     .role(roles::USER)
    ///     .content("Hello")
    ///     .build();
    /// ```
    #[must_use]
    pub fn role(mut self, role: &str) -> Self {
        self.role = Some(role.to_string());
        self
    }

    /// Sets the content for the message.
    ///
    /// # Examples
    /// ```
    /// use weavegraph::message::Message;
    ///
    /// let msg = Message::builder()
    ///     .role("user")
    ///     .content("What time is it?")
    ///     .build();
    /// ```
    #[must_use]
    pub fn content(mut self, content: &str) -> Self {
        self.content = Some(content.to_string());
        self
    }

    /// Builds the message.
    ///
    /// Uses empty strings for any unset fields.
    ///
    /// # Examples
    /// ```
    /// use weavegraph::message::Message;
    ///
    /// let msg = Message::builder()
    ///     .role("user")
    ///     .content("Hello")
    ///     .build();
    ///
    /// assert_eq!(msg.role, "user");
    /// assert_eq!(msg.content, "Hello");
    /// ```
    #[must_use]
    pub fn build(self) -> Message {
        Message {
            role: self.role.unwrap_or_default(),
            content: self.content.unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// Verifies that a Message struct can be constructed and its fields are set correctly.
    fn test_message_construction() {
        let msg = Message {
            role: "user".to_string(),
            content: "hello".to_string(),
        };
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "hello");
    }

    #[test]
    /// Checks that cloning a Message produces an identical copy, and modifying the clone does not affect the original.
    fn test_message_cloning() {
        let msg1 = Message {
            role: "system".to_string(),
            content: "foo".to_string(),
        };
        let msg2 = msg1.clone();
        assert_eq!(msg1, msg2);
        // Changing the clone does not affect the original
        let mut msg2 = msg2;
        msg2.content = "bar".to_string();
        assert_ne!(msg1, msg2);
    }

    #[test]
    /// Validates equality and inequality comparisons for Message structs with different field values.
    fn test_message_equality() {
        let m1 = Message {
            role: "user".to_string(),
            content: "hi".to_string(),
        };
        let m2 = Message {
            role: "user".to_string(),
            content: "hi".to_string(),
        };
        let m3 = Message {
            role: "assistant".to_string(),
            content: "hi".to_string(),
        };
        let m4 = Message {
            role: "user".to_string(),
            content: "bye".to_string(),
        };
        assert_eq!(m1, m2);
        assert_ne!(m1, m3);
        assert_ne!(m1, m4);
    }

    #[test]
    /// Tests convenience constructors for common message types.
    fn test_convenience_constructors() {
        let user_msg = Message::user("Hello");
        assert_eq!(user_msg.role, roles::USER);
        assert_eq!(user_msg.content, "Hello");

        let assistant_msg = Message::assistant("Hi there!");
        assert_eq!(assistant_msg.role, roles::ASSISTANT);
        assert_eq!(assistant_msg.content, "Hi there!");

        let system_msg = Message::system("You are helpful");
        assert_eq!(system_msg.role, roles::SYSTEM);
        assert_eq!(system_msg.content, "You are helpful");

        let custom_msg = Message::new("function", "Result: 42");
        assert_eq!(custom_msg.role, "function");
        assert_eq!(custom_msg.content, "Result: 42");
    }

    #[test]
    /// Tests message builder pattern for fluent construction.
    fn test_message_builder() {
        let msg = Message::builder()
            .role("custom")
            .content("test content")
            .build();

        assert_eq!(msg.role, "custom");
        assert_eq!(msg.content, "test content");

        // Test partial building
        let empty_msg = Message::builder().build();
        assert_eq!(empty_msg.role, "");
        assert_eq!(empty_msg.content, "");

        // Test role-only
        let role_only = Message::builder().role("user").build();
        assert_eq!(role_only.role, "user");
        assert_eq!(role_only.content, "");
    }

    #[test]
    /// Tests message validation functionality.
    fn test_message_validation() {
        let valid_msg = Message::user("Hello world");
        assert!(valid_msg.is_valid());

        let empty_role = Message::new("", "content");
        assert!(!empty_role.is_valid());

        let empty_content = Message::new("user", "");
        assert!(!empty_content.is_valid());

        let whitespace_only = Message::new("  ", "  ");
        assert!(!whitespace_only.is_valid());

        let whitespace_content = Message::new("user", "  hello  ");
        assert!(whitespace_content.is_valid()); // Content has non-whitespace
    }

    #[test]
    /// Tests role checking methods.
    fn test_role_checking() {
        let user_msg = Message::user("Hello");
        assert!(user_msg.is_user());
        assert!(!user_msg.is_assistant());
        assert!(!user_msg.is_system());
        assert!(user_msg.has_role(roles::USER));

        let assistant_msg = Message::assistant("Hi");
        assert!(!assistant_msg.is_user());
        assert!(assistant_msg.is_assistant());
        assert!(!assistant_msg.is_system());
        assert!(assistant_msg.has_role(roles::ASSISTANT));

        let system_msg = Message::system("You are helpful");
        assert!(!system_msg.is_user());
        assert!(!system_msg.is_assistant());
        assert!(system_msg.is_system());
        assert!(system_msg.has_role(roles::SYSTEM));

        let custom_msg = Message::new("function", "result");
        assert!(!custom_msg.is_user());
        assert!(!custom_msg.is_assistant());
        assert!(!custom_msg.is_system());
        assert!(custom_msg.has_role("function"));
    }

    #[test]
    /// Tests role constants are correct.
    fn test_role_constants() {
        assert_eq!(roles::USER, "user");
        assert_eq!(roles::ASSISTANT, "assistant");
        assert_eq!(roles::SYSTEM, "system");
    }

    #[test]
    /// Tests serialization and deserialization.
    fn test_serialization() {
        let original = Message::user("Test message");
        let json = serde_json::to_string(&original).expect("Serialization failed");
        let deserialized: Message = serde_json::from_str(&json).expect("Deserialization failed");

        assert_eq!(original, deserialized);
        assert_eq!(deserialized.role, "user");
        assert_eq!(deserialized.content, "Test message");
    }
}
