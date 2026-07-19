//! Export the resource manifest meta types as TypeScript definitions (via
//! specta) and JSON schemas (via schemars) into `packages/manifest-metadata`.

use std::borrow::Cow;
use std::collections::HashSet;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use manifest_metadata::{ResourceManifest, ResourceManifestTemplate};
use serde_json::Value;
use specta::datatype::{DataType, Field, Fields};
use specta::{Format, FormatError, Types};
use specta_typescript::Typescript;

/// Attribute key recorded by the specta derive for
/// `#[serde(skip_serializing_if = ...)]`, as defined by `specta-serde`.
const FIELD_SKIP_SERIALIZING_IF: &str = "serde:field:skip_serializing_if";

/// TypeScript export shape with these adjustments over `specta_serde::Format`:
///
/// 1. Fields marked with `skip_serializing_if` are reported as optional
///    properties (`field?: T`) instead of being rejected. The unified format
///    refuses the attribute because a unified type graph cannot express
///    conditional omission, and `PhasesFormat` would duplicate every divergent
///    type as `*_Serialize` / `*_Deserialize`. The meta types only use the
///    attribute for optional JSON fields (and mark them `#[specta(optional)]`),
///    so stripping it up front keeps the export faithful.
/// 2. `manifest_version` is exported as the literal `2`, matching
///    `typeof RESOURCE_MANIFEST_VERSION` in the reference definitions.
/// 3. `ResourceChecksum` is exported as the template literal
///    `` `${ChecksumAlgorithm}:${string}` `` instead of a plain string. A
///    `define` reference cannot serve as a named type body (the exporter
///    drops it), so this adjustment happens as an exact text replacement in
///    [`export_typescript`].
/// 4. specta-typescript renders any `f64` as `number | null` because serde_json
///    turns NaN/infinity into `null`. The only float in the graph is
///    `FiniteF64`, which rejects non-finite values by construction, so the
///    semantic lossless-float remap (`number`) is applied.
struct SerdeShape {
    semantic: specta_typescript::semantic::Configuration,
}

impl SerdeShape {
    fn new() -> Self {
        Self {
            semantic: specta_typescript::semantic::Configuration::default()
                .enable_lossless_floats(),
        }
    }

    fn adjust(types: &mut Types) {
        types.iter_mut(|named| {
            if let Some(ty) = named.ty.as_mut() {
                Self::adjust_datatype(ty);
            }
        });
    }

    fn adjust_datatype(ty: &mut DataType) {
        match ty {
            DataType::Struct(strct) => Self::adjust_fields(&mut strct.fields),
            DataType::Enum(enm) => {
                for (_, variant) in &mut enm.variants {
                    Self::adjust_fields(&mut variant.fields);
                }
            }
            _ => {}
        }
    }

    fn adjust_fields(fields: &mut Fields) {
        match fields {
            Fields::Named(named) => {
                for (name, field) in &mut named.fields {
                    Self::adjust_field(name, field);
                }
            }
            Fields::Unnamed(unnamed) => {
                for field in &mut unnamed.fields {
                    Self::adjust_field("", field);
                }
            }
            Fields::Unit => {}
        }
    }

    /// Replace the field's `Nullable(T)` with `T`, leaving any other shape
    /// untouched.
    fn unwrap_nullable(field: &mut Field) {
        if matches!(field.ty, Some(DataType::Nullable(_)))
            && let Some(DataType::Nullable(inner)) = field.ty.take()
        {
            field.ty = Some(*inner);
        }
    }

    fn adjust_field(name: &str, field: &mut Field) {
        if name == "manifest_version" {
            field.ty = Some(DataType::Reference(specta_typescript::define("2")));
        }

        if !field.attributes.contains_key(FIELD_SKIP_SERIALIZING_IF) {
            return;
        }
        field.attributes.remove(FIELD_SKIP_SERIALIZING_IF);
        field.optional = true;
        // The field is omitted (not null) when absent, so export `field?: T`
        // rather than `field?: T | null`.
        Self::unwrap_nullable(field);
    }
}

impl Format for SerdeShape {
    fn map_types<'a>(&'a self, types: &Types) -> Result<Cow<'a, Types>, FormatError> {
        let mut types = types.clone();
        Self::adjust(&mut types);
        let mapped = specta_serde::Format.map_types(&types)?.into_owned();
        // Semantic remapping (lossless floats) runs last, on the serde-shaped
        // graph, as documented by `specta_typescript::semantic`.
        Ok(Cow::Owned(self.semantic.apply_types(&mapped).into_owned()))
    }

    fn map_type<'a>(
        &'a self,
        types: &Types,
        data_type: &DataType,
    ) -> Result<Cow<'a, DataType>, FormatError> {
        let mut data_type = data_type.clone();
        Self::adjust_datatype(&mut data_type);
        specta_serde::Format
            .map_type(types, &data_type)
            .map(|mapped| Cow::Owned(mapped.into_owned()))
    }
}

/// JSON schema transform aligning optional properties with the TypeScript
/// contract: a property that may be absent must not also accept `null`.
///
/// schemars models every `Option<T>` field as nullable; with
/// `skip_serializing_if` (used by all optional manifest fields) the value is
/// omitted when absent and is never `null` on the wire.
///
/// The walk also drops the `format` keyword: schemars emits implementation
/// detail formats (`uint32`, `double`, …) that strict validators like ajv
/// reject as unknown.
fn normalize_schema(schema: &mut schemars::Schema) {
    if let Some(object) = schema.as_object_mut() {
        normalize_schema_map(object);
    }
}

fn normalize_schema_map(object: &mut serde_json::Map<String, Value>) {
    object.shift_remove("format");

    let required: HashSet<String> = object
        .get("required")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();

    if let Some(Value::Object(properties)) = object.get_mut("properties") {
        for (name, property) in properties.iter_mut() {
            if !required.contains(name) {
                strip_null_variant(property);
            }
        }
    }

    for (_, child) in object.iter_mut() {
        normalize_schema_value(child);
    }
}

fn normalize_schema_value(value: &mut Value) {
    match value {
        Value::Object(object) => normalize_schema_map(object),
        Value::Array(items) => {
            for item in items {
                normalize_schema_value(item);
            }
        }
        _ => {}
    }
}

/// Remove the `null` alternative from a property schema, keeping every other
/// keyword (descriptions, nested types) untouched.
fn strip_null_variant(schema: &mut Value) {
    let Some(object) = schema.as_object_mut() else {
        return;
    };

    if let Some(ty) = object.get_mut("type")
        && let Value::Array(types) = ty
    {
        types.retain(|item| item.as_str() != Some("null"));
        if types.len() == 1 {
            *ty = types.pop().expect("one remaining type");
        }
    }

    if let Some(Value::Array(branches)) = object.get_mut("anyOf") {
        branches.retain(|branch| branch.get("type").and_then(Value::as_str) != Some("null"));
    }
}

fn metadata_package_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/manifest-metadata")
}

fn export_typescript(out_dir: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let types = Types::default()
        .register::<ResourceManifest>()
        .register::<ResourceManifestTemplate>();

    let out = out_dir.join("src/types.ts");
    Typescript::default().export_to(&out, &types, SerdeShape::new())?;

    // Export `ResourceChecksum` as the reference template-literal type rather
    // than a plain string alias. The replacement is exact and counted so a
    // changed specta output fails loudly instead of drifting silently.
    const CHECKSUM_ALIAS: &str = "export type ResourceChecksum = string;";
    const CHECKSUM_TEMPLATE: &str =
        "export type ResourceChecksum = `${ChecksumAlgorithm}:${string}`;";
    let bindings = fs::read_to_string(&out)?;
    let replacements = bindings.matches(CHECKSUM_ALIAS).count();
    if replacements != 1 {
        return Err(format!(
            "expected exactly one {CHECKSUM_ALIAS:?} in {}, found {replacements}",
            out.display()
        )
        .into());
    }
    fs::write(&out, bindings.replace(CHECKSUM_ALIAS, CHECKSUM_TEMPLATE))?;
    Ok(out)
}

fn export_json_schema<T: schemars::JsonSchema>(
    out_dir: &Path,
    file_name: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    let settings = schemars::generate::SchemaSettings::draft2020_12().with(|settings| {
        settings.transforms.push(Box::new(normalize_schema));
    });
    let schema = settings.into_generator().into_root_schema_for::<T>();
    let out = out_dir.join("schema").join(file_name);
    fs::write(
        &out,
        format!("{}\n", serde_json::to_string_pretty(&schema)?),
    )?;
    Ok(out)
}

fn main() -> Result<(), Box<dyn Error>> {
    let out_dir = metadata_package_dir();
    fs::create_dir_all(out_dir.join("src"))?;
    fs::create_dir_all(out_dir.join("schema"))?;
    let out_dir = out_dir.canonicalize()?;

    let written = [
        export_typescript(&out_dir)?,
        export_json_schema::<ResourceManifest>(&out_dir, "manifest.schema.json")?,
        export_json_schema::<ResourceManifestTemplate>(&out_dir, "manifest-template.schema.json")?,
    ];

    for path in written {
        println!("wrote {}", path.display());
    }
    Ok(())
}
