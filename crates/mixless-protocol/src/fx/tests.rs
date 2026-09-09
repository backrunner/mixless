use super::*;
#[test]
fn catalogue_ids_names_and_serde_round_trip() {
    for (id, kind) in FxKind::ALL.into_iter().enumerate() {
        assert_eq!(kind as u32, id as u32);
        assert_eq!(FxKind::from_id(id as u32), kind);
        assert_eq!(FxKind::parse(kind.as_str()), Some(kind));
        let json = serde_json::to_string(&FxState::new(kind)).unwrap();
        assert_eq!(
            serde_json::from_str::<FxState>(&json).unwrap(),
            FxState::new(kind)
        );
        assert!(kind.default_beats() <= kind.max_beats());
    }
    assert_eq!(FxKind::AutoPan as u32, 14);
    assert_eq!(FxKind::parse("trans"), Some(FxKind::Gate));
    assert_eq!(FxKind::parse("echo-out"), Some(FxKind::EchoOut));
    assert_eq!(FxKind::parse("unknown"), None);
}
