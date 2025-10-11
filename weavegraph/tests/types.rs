use weavegraph::types::{ChannelType, NodeKind};

#[test]
fn test_nodekind_predicates() {
    assert!(NodeKind::Start.is_start());
    assert!(!NodeKind::Start.is_end());
    assert!(!NodeKind::Start.is_custom());

    assert!(!NodeKind::End.is_start());
    assert!(NodeKind::End.is_end());
    assert!(!NodeKind::End.is_custom());

    let custom = NodeKind::Custom("Test".to_string());
    assert!(!custom.is_start());
    assert!(!custom.is_end());
    assert!(custom.is_custom());
}

#[test]
fn test_nodekind_encode_decode() {
    let test_cases = vec![
        (NodeKind::Start, "Start"),
        (NodeKind::End, "End"),
        (
            NodeKind::Custom("Processor".to_string()),
            "Custom:Processor",
        ),
    ];

    for (node, expected) in test_cases {
        let encoded = node.encode();
        assert_eq!(encoded, expected);

        let decoded = NodeKind::decode(&encoded);
        assert_eq!(decoded, node);
    }
}

#[test]
fn test_display() {
    assert_eq!(NodeKind::Start.to_string(), "Start");
    assert_eq!(NodeKind::End.to_string(), "End");
    assert_eq!(
        NodeKind::Custom("DataProcessor".to_string()).to_string(),
        "DataProcessor"
    );

    assert_eq!(ChannelType::Message.to_string(), "message");
    assert_eq!(ChannelType::Error.to_string(), "error");
    assert_eq!(ChannelType::Extra.to_string(), "extra");
}

#[test]
fn test_serde_support() {
    let nodes = vec![
        NodeKind::Start,
        NodeKind::End,
        NodeKind::Custom("TestNode".to_string()),
    ];
    for node in nodes {
        let serialized = serde_json::to_string(&node).unwrap();
        let deserialized: NodeKind = serde_json::from_str(&serialized).unwrap();
        assert_eq!(node, deserialized);
    }

    let channels = vec![ChannelType::Message, ChannelType::Error, ChannelType::Extra];
    for channel in channels {
        let serialized = serde_json::to_string(&channel).unwrap();
        let deserialized: ChannelType = serde_json::from_str(&serialized).unwrap();
        assert_eq!(channel, deserialized);
    }
}
