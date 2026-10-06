//! Masking an already-serialized [`crate::value::Value`] in place.
//!
//! Milestone 3i, decision (b4). [`crate::value::Value::display`] masks a
//! live `Value` before it is rendered to text; [`mask_json`] does the same
//! job after a `Value` (or a whole structure holding many of them) has
//! already become [`serde_json::Value`] — the CLI's `--json`, an MCP
//! result, a journal-sourced `RunRecord`, and the approvals page all
//! reach their JSON this way, often by deserializing something built
//! before any output mode existed (a recorded [`crate::plan::Plan`] in
//! the journal, say). A post-pass over the JSON tree is the one
//! chokepoint that reaches every one of those alike.

use willikins_types::{TypeName, mask_identifier};

/// Walk `value` in place and mask every nested object that is a *known*
/// [`crate::value::Value`] of a registered identifier type.
///
/// Recurses into every object's values and every array's elements, so it
/// reaches a `Value` nested anywhere inside a larger JSON document (a
/// `Plan`, a `Description`, a `RunRecord`, ...), not only a bare `Value`
/// at the top. An object is treated as `Value`'s own wire shape — and so
/// is a candidate for masking — only when it carries all four of:
/// - `"type"`, a JSON string (the type name);
/// - `"list"`, a JSON boolean;
/// - `"state"`, the JSON string `"known"` (an `"unknown"` state carries no
///   `value` to mask, and is left alone);
/// - `"value"`, present at all (its own shape — string or array of
///   strings — is `Value::Serialize`'s to decide, not this function's).
///
/// A schema-shaped object — `"type"` without `"list"`/`"state"` (the shape
/// `MissingInput::schema` or a tool's `outputSchema` uses) — fails that
/// test and is left alone, though this function still recurses into it in
/// case it nests a real `Value` somewhere inside (it structurally cannot
/// today, but `mask_json` does not assume that).
///
/// Whether a matching object is actually masked is decided by the
/// registry, not by this function's own list of names: `"type"`'s string
/// is looked up with [`willikins_types::registry`], and masking happens
/// only when that registered type's
/// [`willikins_types::TypeInfo::identifier`] is `true`. An unregistered
/// type name, or a `"type"` that happens to be some other string
/// entirely, is left untouched rather than erred on — this function
/// never fails, because every caller runs it over data that already
/// passed `Value`'s own `Serialize`, and over journal/approval JSON that
/// must still render even if a type was since removed from the registry.
///
/// Masking a matching object replaces its `"value"` — a string, masked
/// directly, or an array of strings, masked element-wise — with
/// [`mask_identifier`]'s prefix form, and adds `"masked": true` beside it.
/// A secret-typed value is never a candidate: the registry never marks a
/// secret type's [`willikins_types::TypeInfo::identifier`] `true` (the
/// derive makes `#[domain(secret, identifier)]` a compile error), so this
/// function finds nothing to mask on one and leaves its `"redacted":
/// true` exactly as `Value::Serialize` wrote it.
pub fn mask_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            if is_known_value_shape(map) {
                mask_known_value(map);
            }
            for nested in map.values_mut() {
                mask_json(nested);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items.iter_mut() {
                mask_json(item);
            }
        }
        _ => {}
    }
}

/// Whether `map` has the shape `Value::Serialize` emits for a *known*
/// value: `"type"` a string, `"list"` a bool, `"state"` exactly
/// `"known"`, and a `"value"` key present (whatever shape it holds).
fn is_known_value_shape(map: &serde_json::Map<String, serde_json::Value>) -> bool {
    matches!(map.get("type"), Some(serde_json::Value::String(_)))
        && matches!(map.get("list"), Some(serde_json::Value::Bool(_)))
        && matches!(map.get("state"), Some(serde_json::Value::String(state)) if state == "known")
        && map.contains_key("value")
}

/// Mask `map`'s `"value"` and add `"masked": true`, but only when
/// `"type"` names a registered identifier type. Does nothing — not even
/// looking at `"value"` — otherwise, so a non-identifier `Value` (and a
/// secret one, which is never marked an identifier) is left byte-for-byte
/// as `Value::Serialize` wrote it.
fn mask_known_value(map: &mut serde_json::Map<String, serde_json::Value>) {
    let Some(type_name) = map.get("type").and_then(|v| v.as_str()) else {
        return;
    };
    let Ok(type_name) = TypeName::parse(type_name) else {
        return;
    };
    let is_identifier = willikins_types::registry()
        .get(&type_name)
        .is_some_and(|entry| entry.info.identifier);
    if !is_identifier {
        return;
    }

    match map.get_mut("value") {
        Some(serde_json::Value::String(s)) => {
            *s = mask_identifier(s);
        }
        Some(serde_json::Value::Array(items)) => {
            for item in items.iter_mut() {
                if let serde_json::Value::String(s) = item {
                    *s = mask_identifier(s);
                }
            }
        }
        _ => return,
    }
    map.insert("masked".to_string(), serde_json::Value::Bool(true));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;
    use serde_json::json;
    use willikins_types::DomainType;

    /// One known, scalar [`Value`] of every registered identifier type,
    /// built from the type's own registry example, plus the matching
    /// known list of two copies of it — the full set decision (b2) pins,
    /// read back through the registry so a widened identifier set is
    /// covered automatically with no change to this test.
    fn every_identifier_type_value() -> Vec<(String, serde_json::Value, serde_json::Value)> {
        willikins_types::registry()
            .iter()
            .filter(|entry| entry.info.identifier)
            .map(|entry| {
                let name = entry.info.name.to_string();
                let scalar = willikins_types::registry()
                    .parse(&TypeName::parse(&name).unwrap(), entry.info.example)
                    .unwrap_or_else(|err| {
                        panic!(
                            "{name}'s own example `{}` must parse: {err}",
                            entry.info.example
                        )
                    });
                let scalar_value = Value::known_dyn_as(
                    willikins_types::registry(),
                    TypeName::parse(&name).unwrap(),
                    scalar.clone(),
                )
                .unwrap_or_else(|err| {
                    panic!("{name}'s own example must match its own registered type: {err:?}")
                });
                let list_value = crate::value::Value::known_dyn_list(
                    TypeName::parse(&name).unwrap(),
                    vec![scalar.clone(), scalar],
                );
                (
                    name,
                    serde_json::to_value(&scalar_value).unwrap(),
                    serde_json::to_value(&list_value).unwrap(),
                )
            })
            .collect()
    }

    #[test]
    fn registry_has_at_least_the_seven_pinned_identifier_types() {
        // Guards this test file's own fixture against the registry
        // reporting zero identifier types (a silently vacuous test).
        assert!(every_identifier_type_value().len() >= 7);
    }

    #[test]
    fn mask_json_masks_a_known_scalar_of_every_identifier_type() {
        for (name, mut scalar_json, _) in every_identifier_type_value() {
            let original_value = scalar_json["value"].as_str().unwrap().to_string();
            mask_json(&mut scalar_json);
            let masked = scalar_json["value"].as_str().unwrap();
            assert_ne!(masked, &original_value, "{name} was not masked");
            assert!(
                masked.ends_with("..."),
                "{name}'s masked value `{masked}` must end in `...`"
            );
            assert!(
                original_value.starts_with(masked.trim_end_matches("...")),
                "{name}'s masked value `{masked}` must be a prefix of `{original_value}`"
            );
            assert_eq!(scalar_json["masked"], json!(true));
        }
    }

    #[test]
    fn mask_json_masks_every_element_of_a_known_list_of_every_identifier_type() {
        for (name, _, mut list_json) in every_identifier_type_value() {
            let originals: Vec<String> = list_json["value"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect();
            mask_json(&mut list_json);
            let masked: Vec<&str> = list_json["value"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            assert_eq!(masked.len(), originals.len());
            for (original, masked) in originals.iter().zip(masked.iter()) {
                assert_ne!(masked, original, "{name}'s list element was not masked");
                assert!(masked.ends_with("..."));
            }
            assert_eq!(list_json["masked"], json!(true));
        }
    }

    #[test]
    fn mask_json_leaves_a_non_identifier_value_untouched() {
        let value = Value::known(willikins_types::GitHubOrg::parse("lightless-labs").unwrap());
        let mut json = serde_json::to_value(&value).unwrap();
        let before = json.clone();
        mask_json(&mut json);
        assert_eq!(json, before);
        assert!(json.get("masked").is_none());
    }

    #[test]
    fn mask_json_leaves_a_secret_value_untouched() {
        const EXAMPLE_TOKEN: &str =
            concat!("dp.st.prd.", "exampleexampleexampleexampleexampleexample");
        let value =
            Value::known(willikins_types::DopplerServiceToken::parse(EXAMPLE_TOKEN).unwrap());
        let mut json = serde_json::to_value(&value).unwrap();
        let before = json.clone();
        mask_json(&mut json);
        assert_eq!(json, before);
        assert!(json.get("masked").is_none());
        assert_eq!(json["redacted"], serde_json::json!(true));
    }

    #[test]
    fn mask_json_leaves_an_unknown_identifier_value_untouched() {
        // An `Unknown` state has no `"value"` to mask: `is_known_value_shape`
        // requires `"state": "known"`.
        let value = Value::unknown(crate::value::TypeRef::scalar(
            TypeName::parse("AppleIssuerId").unwrap(),
        ));
        let mut json = serde_json::to_value(&value).unwrap();
        let before = json.clone();
        mask_json(&mut json);
        assert_eq!(json, before);
    }

    /// A schema-shaped object (`"type"` without `"list"`/`"state"` — the
    /// shape `MissingInput::schema` and a published `outputSchema` use)
    /// must never be mistaken for a `Value` and masked, even when its
    /// `"type"` string happens to equal a real identifier type's name.
    #[test]
    fn mask_json_leaves_a_schema_shaped_object_untouched() {
        let mut schema_shaped = json!({
            "type": "AppleIssuerId",
            "description": "not a Value at all",
        });
        let before = schema_shaped.clone();
        mask_json(&mut schema_shaped);
        assert_eq!(schema_shaped, before);
    }

    /// `mask_json` recurses into a larger structure — an array of
    /// `Value`s, and a `Value` nested inside another object's field — not
    /// only a bare top-level `Value`.
    #[test]
    fn mask_json_recurses_into_arrays_and_nested_objects() {
        let issuer = Value::known(
            willikins_types::AppleIssuerId::parse("57246542-96fe-1a63-e053-0824d011072a").unwrap(),
        );
        let mut nested = json!({
            "outputs": [serde_json::to_value(&issuer).unwrap()],
            "node": { "issuer_id": serde_json::to_value(&issuer).unwrap() },
        });
        mask_json(&mut nested);
        assert_eq!(nested["outputs"][0]["value"], json!("5724..."));
        assert_eq!(nested["outputs"][0]["masked"], json!(true));
        assert_eq!(nested["node"]["issuer_id"]["value"], json!("5724..."));
        assert_eq!(nested["node"]["issuer_id"]["masked"], json!(true));
    }

    #[test]
    fn mask_json_is_a_no_op_on_scalars() {
        let mut n = json!(42);
        mask_json(&mut n);
        assert_eq!(n, json!(42));
        let mut s = json!("hello");
        mask_json(&mut s);
        assert_eq!(s, json!("hello"));
        let mut null = serde_json::Value::Null;
        mask_json(&mut null);
        assert!(null.is_null());
    }
}
