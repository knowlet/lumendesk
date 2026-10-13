//! Explicit schemas use JavaScript-safe integers. Optional object properties and
//! nullable values are distinct. Maps are ordered so generated files are stable.
use crate::{CoreError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Schema {
    Json,
    Null,
    Boolean,
    Number,
    Integer,
    String,
    Array {
        items: Box<Schema>,
    },
    Nullable {
        inner: Box<Schema>,
    },
    Object {
        fields: BTreeMap<String, Field>,
        additional: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub schema: Schema,
    pub optional: bool,
}

impl Field {
    pub fn required(schema: Schema) -> Self {
        Self {
            schema,
            optional: false,
        }
    }
    pub fn optional(schema: Schema) -> Self {
        Self {
            schema,
            optional: true,
        }
    }
}

impl Schema {
    pub fn array(items: Schema) -> Self {
        Self::Array {
            items: Box::new(items),
        }
    }
    pub fn nullable(inner: Schema) -> Self {
        Self::Nullable {
            inner: Box::new(inner),
        }
    }
    pub fn object(fields: impl IntoIterator<Item = (String, Field)>) -> Self {
        Self::Object {
            fields: fields.into_iter().collect(),
            additional: false,
        }
    }

    /// Validate values at both sides of a typed registration. `Integer` rejects
    /// values outside ±(2^53−1), even when Rust's target integer could hold them.
    pub fn validate(&self, value: &Value) -> std::result::Result<(), String> {
        self.validate_at(value, "$", 0)
    }

    fn validate_at(
        &self,
        value: &Value,
        path: &str,
        depth: usize,
    ) -> std::result::Result<(), String> {
        if depth > 64 {
            return Err(format!("{path}: nesting exceeds 64"));
        }
        let bad = || Err(format!("{path}: expected {}", self.typescript()));
        match self {
            Self::Json => Ok(()),
            Self::Null if value.is_null() => Ok(()),
            Self::Boolean if value.is_boolean() => Ok(()),
            Self::String if value.is_string() => Ok(()),
            Self::Number if value.is_number() => Ok(()),
            Self::Integer => {
                const SAFE: i64 = 9_007_199_254_740_991;
                if value.as_i64().is_some_and(|n| (-SAFE..=SAFE).contains(&n))
                    || value.as_u64().is_some_and(|n| n <= SAFE as u64)
                {
                    Ok(())
                } else {
                    bad()
                }
            }
            Self::Nullable { inner } => {
                if value.is_null() {
                    Ok(())
                } else {
                    inner.validate_at(value, path, depth + 1)
                }
            }
            Self::Array { items } => {
                let Some(values) = value.as_array() else {
                    return bad();
                };
                for (i, child) in values.iter().enumerate() {
                    items.validate_at(child, &format!("{path}[{i}]"), depth + 1)?;
                }
                Ok(())
            }
            Self::Object { fields, additional } => {
                let Some(values) = value.as_object() else {
                    return bad();
                };
                for (name, field) in fields {
                    match values.get(name) {
                        Some(child) => {
                            field
                                .schema
                                .validate_at(child, &format!("{path}.{name}"), depth + 1)?
                        }
                        None if !field.optional => {
                            return Err(format!("{path}.{name}: required property missing"));
                        }
                        None => (),
                    }
                }
                if !additional
                    && let Some(name) = values.keys().find(|name| !fields.contains_key(*name))
                {
                    return Err(format!("{path}.{name}: unknown property"));
                }
                Ok(())
            }
            _ => bad(),
        }
    }

    pub fn typescript(&self) -> String {
        match self {
            Self::Json => "JsonValue".into(),
            Self::Null => "null".into(),
            Self::Boolean => "boolean".into(),
            Self::Number | Self::Integer => "number".into(),
            Self::String => "string".into(),
            Self::Array { items } => format!("Array<{}>", items.typescript()),
            Self::Nullable { inner } => format!("({} | null)", inner.typescript()),
            Self::Object { fields, additional } => {
                let mut output = String::from("{ ");
                for (name, field) in fields {
                    output.push_str(&format!(
                        "{}{}: {}; ",
                        quote(name),
                        if field.optional { "?" } else { "" },
                        field.schema.typescript()
                    ));
                }
                if *additional {
                    output.push_str("[key: string]: JsonValue | undefined; ");
                }
                output.push('}');
                output
            }
        }
    }
}

fn quote(text: &str) -> String {
    serde_json::to_string(text).expect("string JSON serialization cannot fail")
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MethodSchema {
    pub name: String,
    pub arguments: Schema,
    pub result: Schema,
}

impl MethodSchema {
    pub fn new(name: impl Into<String>, arguments: Schema, result: Schema) -> Self {
        Self {
            name: name.into(),
            arguments,
            result,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaRegistry {
    #[serde(default)]
    methods: BTreeMap<String, MethodSchema>,
    #[serde(default)]
    events: BTreeMap<String, Schema>,
}

impl SchemaRegistry {
    pub fn register_method(&mut self, method: MethodSchema) -> Result<()> {
        validate_name(&method.name)?;
        if self.methods.contains_key(&method.name) {
            return Err(CoreError::DuplicateRegistration(method.name));
        }
        self.methods.insert(method.name.clone(), method);
        Ok(())
    }
    pub fn register_event(&mut self, name: impl Into<String>, payload: Schema) -> Result<()> {
        let name = name.into();
        validate_name(&name)?;
        if self.events.contains_key(&name) {
            return Err(CoreError::DuplicateRegistration(name));
        }
        self.events.insert(name, payload);
        Ok(())
    }
    pub fn methods(&self) -> &BTreeMap<String, MethodSchema> {
        &self.methods
    }
    pub fn events(&self) -> &BTreeMap<String, Schema> {
        &self.events
    }
    pub fn typescript(&self) -> String {
        let mut text = String::from(
            "// Generated by LumenDesk. Do not edit.\n// Integer schemas require safe integers; encode larger integers as strings.\nexport type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };\nexport interface Methods {\n",
        );
        for (name, method) in &self.methods {
            text.push_str(&format!(
                "  {}: {{ args: {}; result: {} }};\n",
                quote(name),
                method.arguments.typescript(),
                method.result.typescript()
            ));
        }
        text.push_str("}\nexport interface Events {\n");
        for (name, payload) in &self.events {
            text.push_str(&format!("  {}: {};\n", quote(name), payload.typescript()));
        }
        text.push_str("}\nexport interface Client {\n  call<K extends keyof Methods>(method: K, args: Methods[K]['args'], signal?: AbortSignal): Promise<Methods[K]['result']>;\n  on<K extends keyof Events>(event: K, listener: (payload: Events[K]) => void, options?: { once?: boolean }): () => void;\n}\n");
        text
    }
}

pub(crate) fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
        return Err(CoreError::InvalidConfiguration(
            "names must contain 1–128 non-control bytes".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn optional_is_not_nullable_and_unknown_fields_fail() {
        let schema = Schema::object([
            ("name".into(), Field::required(Schema::String)),
            ("age".into(), Field::optional(Schema::Integer)),
        ]);
        assert!(schema.validate(&json!({"name":"Orion"})).is_ok());
        assert!(
            schema
                .validate(&json!({"name":"Orion", "age":null}))
                .is_err()
        );
        assert!(schema.validate(&json!({"age":1})).is_err());
        assert!(
            schema
                .validate(&json!({"name":"Orion", "extra":1}))
                .is_err()
        );
        assert!(
            Schema::nullable(Schema::String)
                .validate(&Value::Null)
                .is_ok()
        );
    }
    #[test]
    fn rejects_unsafe_and_fractional_integers() {
        assert!(
            Schema::Integer
                .validate(&json!(9_007_199_254_740_991u64))
                .is_ok()
        );
        assert!(
            Schema::Integer
                .validate(&json!(9_007_199_254_740_992u64))
                .is_err()
        );
        assert!(
            Schema::Integer
                .validate(&json!(-9_007_199_254_740_992i64))
                .is_err()
        );
        assert!(Schema::Integer.validate(&json!(1.5)).is_err());
    }
    #[test]
    fn output_is_deterministic_and_quotes_arbitrary_names() {
        let methods = [
            MethodSchema::new("z", Schema::Null, Schema::String),
            MethodSchema::new("a\"b", Schema::Boolean, Schema::Boolean),
        ];
        let mut a = SchemaRegistry::default();
        let mut b = SchemaRegistry::default();
        for method in &methods {
            a.register_method(method.clone()).unwrap();
        }
        for method in methods.iter().rev() {
            b.register_method(method.clone()).unwrap();
        }
        assert_eq!(a.typescript(), b.typescript());
        assert!(a.typescript().contains("\"a\\\"b\""));
        assert!(a.register_method(methods[0].clone()).is_err());
    }
}
