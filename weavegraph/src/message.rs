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
/// ## Basic Construction
/// ```
/// use weavegraph::message::Message;
///
/// // Manual construction
/// let message = Message {
///     role: Message::USER.to_string(),
///     content: "Hello, world!".to_string(),
/// };
///
/// // Using convenience constructors (recommended)
/// let user_msg = Message::user("What is the weather?");
/// let assistant_msg = Message::assistant("It's sunny today!");
/// let system_msg = Message::system("You are a helpful assistant.");
/// ```
///
/// ## Ergonomic From Trait Conversions
/// ```
/// use weavegraph::message::Message;
///
/// // Convert string slice to user message (most common case)
/// let msg1: Message = "Hello!".into();
/// assert_eq!(msg1.role, Message::USER);
///
/// // Convert String to user message  
/// let content = String::from("Dynamic content");
/// let msg2: Message = content.into();
///
/// // Convert (role, content) tuple for any role
/// let msg3: Message = ("function", "Processing complete").into();
/// let msg4: Message = (Message::ASSISTANT, "How can I help?").into();
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
    ///
    /// # Examples
    /// ```
    /// use weavegraph::message::Message;
    ///
    /// let msg = Message::new(Message::USER, "Hello!");
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
        Self::new(Self::USER, content)
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
        Self::new(Self::ASSISTANT, content)
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
        Self::new(Self::SYSTEM, content)
    }

    /// Returns true if this message has the specified role.
    ///
    /// # Examples
    /// ```
    /// use weavegraph::message::Message;
    ///
    /// let msg = Message::user("Hello");
    /// assert!(msg.has_role(Message::USER));
    /// assert!(!msg.has_role(Message::ASSISTANT));
    /// ```
    #[must_use]
    pub fn has_role(&self, role: &str) -> bool {
        self.role == role
    }

    /// Returns the content length in characters.
    ///
    /// # Examples
    /// ```
    /// use weavegraph::message::Message;
    ///
    /// let msg = Message::user("Hello");
    /// assert_eq!(msg.len(), 5);
    /// ```
    #[must_use]
    pub fn len(&self) -> usize {
        self.content.len()
    }

    /// Returns true if the message content is empty.
    ///
    /// # Examples
    /// ```
    /// use weavegraph::message::Message;
    ///
    /// let empty_msg = Message::user("");
    /// assert!(empty_msg.is_empty());
    ///
    /// let msg = Message::user("Hello");
    /// assert!(!msg.is_empty());
    /// ```
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }
}

/// Ergonomic From trait implementations for common conversions.
impl From<&str> for Message {
    /// Convert a string slice into a user message.
    ///
    /// This provides the most ergonomic way to create messages for the common case
    /// where you want to create a user message from a string.
    ///
    /// # Examples
    /// ```
    /// use weavegraph::message::Message;
    ///
    /// let msg: Message = "Hello, world!".into();
    /// assert_eq!(msg.role, Message::USER);
    /// assert_eq!(msg.content, "Hello, world!");
    /// ```
    fn from(content: &str) -> Self {
        Self::user(content)
    }
}

impl From<String> for Message {
    /// Convert a String into a user message.
    ///
    /// # Examples
    /// ```
    /// use weavegraph::message::Message;
    ///
    /// let content = String::from("Hello, world!");
    /// let msg: Message = content.into();
    /// assert_eq!(msg.role, Message::USER);
    /// assert_eq!(msg.content, "Hello, world!");
    /// ```
    fn from(content: String) -> Self {
        Self::user(&content)
    }
}

impl From<(&str, &str)> for Message {
    /// Convert a (role, content) tuple into a message.
    ///
    /// # Examples
    /// ```
    /// use weavegraph::message::Message;
    ///
    /// let msg: Message = ("assistant", "Hello there!").into();
    /// assert_eq!(msg.role, "assistant");
    /// assert_eq!(msg.content, "Hello there!");
    ///
    /// // Using constants
    /// let msg: Message = (Message::SYSTEM, "You are helpful").into();
    /// assert_eq!(msg.role, Message::SYSTEM);
    /// ```
    fn from((role, content): (&str, &str)) -> Self {
        Self::new(role, content)
    }
}

impl std::fmt::Display for Message {
    /// Format a message for display.
    ///
    /// Shows the role and content in a readable format.
    ///
    /// # Examples
    /// ```
    /// use weavegraph::message::Message;
    ///
    /// let msg = Message::user("Hello, world!");
    /// assert_eq!(format!("{}", msg), "user: Hello, world!");
    ///
    /// let msg = Message::assistant("How can I help?");
    /// assert_eq!(format!("{}", msg), "assistant: How can I help?");
    /// ```
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.role, self.content)
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
    /// Tests role checking methods.
    fn test_role_checking() {
        let user_msg = Message::user("Hello");
        assert!(user_msg.has_role(Message::USER));
        assert!(!user_msg.has_role(Message::ASSISTANT));
        assert!(!user_msg.has_role(Message::SYSTEM));

        let assistant_msg = Message::assistant("Hi");
        assert!(!assistant_msg.has_role(Message::USER));
        assert!(assistant_msg.has_role(Message::ASSISTANT));
        assert!(!assistant_msg.has_role(Message::SYSTEM));

        let system_msg = Message::system("You are helpful");
        assert!(!system_msg.has_role(Message::USER));
        assert!(!system_msg.has_role(Message::ASSISTANT));
        assert!(system_msg.has_role(Message::SYSTEM));

        let custom_msg = Message::new("function", "result");
        assert!(!custom_msg.has_role(Message::USER));
        assert!(!custom_msg.has_role(Message::ASSISTANT));
        assert!(!custom_msg.has_role(Message::SYSTEM));
        assert!(custom_msg.has_role("function"));
    }

    #[test]
    /// Tests role constants are correct.
    fn test_role_constants() {
        assert_eq!(Message::USER, "user");
        assert_eq!(Message::ASSISTANT, "assistant");
        assert_eq!(Message::SYSTEM, "system");
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

    #[test]
    /// Tests From trait implementations for ergonomic conversions.
    fn test_from_implementations() {
        // From &str - creates user message
        let msg1: Message = "Hello world".into();
        assert_eq!(msg1.role, Message::USER);
        assert_eq!(msg1.content, "Hello world");

        // From String - creates user message
        let content = String::from("Hello from String");
        let msg2: Message = content.into();
        assert_eq!(msg2.role, Message::USER);
        assert_eq!(msg2.content, "Hello from String");

        // From (&str, &str) tuple - creates message with specified role
        let msg3: Message = ("assistant", "Assistant response").into();
        assert_eq!(msg3.role, "assistant");
        assert_eq!(msg3.content, "Assistant response");

        // From tuple with constant
        let msg4: Message = (Message::SYSTEM, "System prompt").into();
        assert_eq!(msg4.role, Message::SYSTEM);
        assert_eq!(msg4.content, "System prompt");
    }

    #[test]
    /// Tests helper methods for message introspection.
    fn test_helper_methods() {
        let short_msg = Message::user("Hi");
        let empty_msg = Message::user("");
        let long_msg = Message::assistant("This is a longer message with more content");

        // Test len()
        assert_eq!(short_msg.len(), 2);
        assert_eq!(empty_msg.len(), 0);
        assert_eq!(long_msg.len(), 42); // Corrected length

        // Test is_empty()
        assert!(!short_msg.is_empty());
        assert!(empty_msg.is_empty());
        assert!(!long_msg.is_empty());
    }

    #[test]
    /// Tests Display trait implementation.
    fn test_display() {
        let user_msg = Message::user("Hello, world!");
        assert_eq!(format!("{}", user_msg), "user: Hello, world!");

        let assistant_msg = Message::assistant("How can I help?");
        assert_eq!(format!("{}", assistant_msg), "assistant: How can I help?");

        let system_msg = Message::system("You are helpful");
        assert_eq!(format!("{}", system_msg), "system: You are helpful");

        let custom_msg = Message::new("function", "Processing complete");
        assert_eq!(format!("{}", custom_msg), "function: Processing complete");
    }
}
