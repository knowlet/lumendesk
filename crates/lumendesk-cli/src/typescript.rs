use crate::Result;
use lumendesk_core::{Field, MethodSchema, Schema, SchemaRegistry};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryFile {
    methods: BTreeMap<String, MethodSchema>,
    #[serde(default)]
    events: BTreeMap<String, Schema>,
}

/// Generate a deterministic client using the same schema engine as the runtime.
pub fn generate_typescript(value: &Value) -> Result<String> {
    let source: RegistryFile = serde_json::from_value(value.clone())
        .map_err(|error| format!("invalid schema registry: {error}"))?;
    let mut registry = SchemaRegistry::default();
    for (key, method) in source.methods {
        if key != method.name {
            return Err(format!(
                "method key '{key}' does not match its name '{}'",
                method.name
            ));
        }
        registry
            .register_method(method)
            .map_err(|error| error.to_string())?;
    }
    for (name, schema) in source.events {
        registry
            .register_event(name, schema)
            .map_err(|error| error.to_string())?;
    }
    Ok(registry.typescript())
}

/// A newly authored form schema, shared by both scaffold modes.
pub fn sample_schema() -> Value {
    let arguments = Schema::object([
        ("name".into(), Field::required(Schema::String)),
        ("enthusiastic".into(), Field::required(Schema::Boolean)),
    ]);
    let result = Schema::object([
        ("name".into(), Field::required(Schema::String)),
        ("enthusiastic".into(), Field::required(Schema::Boolean)),
        ("greeting".into(), Field::required(Schema::String)),
        ("submissions".into(), Field::required(Schema::Integer)),
    ]);
    let method = MethodSchema::new("greet", arguments, result.clone());
    serde_json::json!({"methods": {"greet": method}, "events": {"greeting": result}})
}
