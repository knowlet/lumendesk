use lumendesk_core::SchemaRegistry;

#[test]
fn generated_typescript_matches_checked_fixture() {
    let registry: SchemaRegistry =
        serde_json::from_str(include_str!("typescript/schema.json")).unwrap();
    let generated = registry.typescript();
    assert_eq!(generated, include_str!("typescript/client.d.ts"));
    assert_eq!(generated, registry.typescript());
    let roundtrip: SchemaRegistry =
        serde_json::from_value(serde_json::to_value(&registry).unwrap()).unwrap();
    assert_eq!(generated, roundtrip.typescript());
}
