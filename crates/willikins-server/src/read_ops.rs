//! Types for [`crate::Butler`]'s read-only operations: `validate`,
//! `describe`, `list_tools`, `propose_slug`. The operations themselves
//! live on `Butler` in `butler.rs` (they need its private rate-limiter
//! fields); this module holds only the shapes their callers see.

use willikins_core::{CheckError, CheckWarning, Reported};
use willikins_types::ProjectSlug;

/// Where a document body comes from for `validate`/`describe`: either
/// supplied inline (the authoring loop -- pure, no provider call, no
/// trust implication) or a name resolved in the trusted workflow
/// directory. `plan`/`apply` accept only [`willikins_types::WorkflowName`]
/// directly -- never a [`Self::Body`] -- which is what makes them unable
/// to run untrusted document text at all; see the plan's "Documents"
/// trust boundary.
#[derive(Debug, Clone)]
pub enum DocumentSource {
    /// An inline document body, not yet known to be in the trusted
    /// directory (or not meant to be: the authoring loop).
    Body(String),
    /// A name to resolve in the trusted workflow directory.
    Name(willikins_types::WorkflowName),
}

/// What [`crate::Butler::validate`] returns.
///
/// `errors` and `warnings` each carry through [`Reported`] on the wire --
/// the same way the CLI's own `validate --json` already does (see
/// `willikins-cli`'s `render` module) -- so every element an agent reads
/// back has both `kind` (its own internal tag) and `message` (its own
/// [`std::fmt::Display`] rendering), never `kind` alone. Closes item 5 of
/// `todos/2026-09-12-error-json-uniformity-gaps.md`; the plan's task 10a
/// addendum records the decision.
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct ValidateResponse {
    /// Whether the document checked cleanly (no errors; warnings are
    /// still allowed).
    pub ok: bool,
    /// Every `check` failure, when `ok` is `false`. Each element
    /// serializes as [`CheckError`]'s own `{"kind", ...fields}` shape plus
    /// `message` -- see the struct's own doc.
    #[serde(serialize_with = "serialize_reported")]
    #[schemars(schema_with = "reported_array_schema::<CheckError>")]
    pub errors: Vec<CheckError>,
    /// Every `check` warning, when `ok` is `true` (a document that fails
    /// `check` reports its failures as `errors`, not warnings, so the two
    /// lists are never both non-empty). Each element carries `message`
    /// too, the same way `errors` does.
    #[serde(serialize_with = "serialize_reported")]
    #[schemars(schema_with = "reported_array_schema::<CheckWarning>")]
    pub warnings: Vec<CheckWarning>,
}

/// Serialize `items` as a JSON array of [`Reported`]-wrapped elements:
/// each one's own `{"kind", ...fields}` shape plus `message`. The
/// `serde(serialize_with = ...)` counterpart of [`reported_array_schema`].
///
/// `pub(crate)` rather than private: `crate::error::ButlerError::Check`
/// and `::Input` (task 11) reuse it for the same reason `ValidateResponse`
/// does -- see this module's doc and
/// `todos/2026-09-12-error-json-uniformity-gaps.md`.
pub(crate) fn serialize_reported<T, S>(items: &[T], serializer: S) -> Result<S::Ok, S::Error>
where
    T: serde::Serialize + std::fmt::Display,
    S: serde::Serializer,
{
    serializer.collect_seq(items.iter().map(Reported::new))
}

/// The schema for a field [`serialize_reported`] serializes: `T`'s own
/// generated schema, `allOf` an object requiring a `message: string` --
/// matching what [`serialize_reported`] actually emits, rather than the
/// bare `Vec<T>` schema `#[derive(JsonSchema)]` would otherwise publish.
fn reported_array_schema<T: schemars::JsonSchema>(
    generator: &mut schemars::SchemaGenerator,
) -> schemars::Schema {
    let inner = generator.subschema_for::<T>();
    schemars::json_schema!({
        "type": "array",
        "items": {
            "allOf": [
                inner,
                {
                    "type": "object",
                    "properties": { "message": { "type": "string" } },
                    "required": ["message"],
                },
            ],
        },
    })
}

/// What [`crate::Butler::propose_slug`] returns on success.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
pub struct ProposeSlugResponse {
    /// The proposed slug.
    pub slug: ProjectSlug,
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::{InputName, NodeName};

    fn input(name: &str) -> InputName {
        InputName::parse(name).unwrap()
    }

    fn node(name: &str) -> NodeName {
        NodeName::parse(name).unwrap()
    }

    /// Closes item 5 of `todos/2026-09-12-error-json-uniformity-gaps.md`:
    /// every element of `errors` and `warnings` carries both `kind` (its
    /// own internal tag, unaffected) and `message` (new), not `kind`
    /// alone. `crates/willikins-cli/tests/acceptance_11_parity.rs`'s
    /// `check_failure_parity` pins this against the CLI's own JSON for a
    /// real fixture; this test pins the shape directly, including a
    /// `CheckWarning`, which no shipped fixture currently exercises.
    #[test]
    fn errors_and_warnings_each_carry_kind_and_message() {
        let response = ValidateResponse {
            ok: false,
            errors: vec![CheckError::Cycle {
                nodes: vec![node("a")],
            }],
            warnings: vec![CheckWarning::UnusedInput {
                input: input("unused"),
            }],
        };
        let json = serde_json::to_value(&response).unwrap();

        let error = &json["errors"][0];
        assert_eq!(error["kind"], "Cycle");
        assert!(error["message"].is_string(), "{error}");

        let warning = &json["warnings"][0];
        assert_eq!(warning["kind"], "UnusedInput");
        assert_eq!(warning["input"], "unused");
        assert!(warning["message"].is_string(), "{warning}");
    }

    /// The generated schema for `errors`/`warnings` actually requires
    /// `message`, matching what `Serialize` emits -- not the bare
    /// `Vec<CheckError>`/`Vec<CheckWarning>` schema
    /// `#[derive(JsonSchema)]` would otherwise have published.
    #[test]
    fn the_generated_schema_requires_message_on_each_element() {
        let schema = schemars::schema_for!(ValidateResponse);
        let value = schema.as_value();
        let errors_schema = &value["properties"]["errors"]["items"];
        let required = errors_schema["allOf"][1]["required"]
            .as_array()
            .expect("an array of required field names");
        assert!(
            required.iter().any(|field| field == "message"),
            "{errors_schema}"
        );
    }
}
