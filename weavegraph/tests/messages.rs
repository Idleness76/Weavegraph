use weavegraph::message::Message;

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
