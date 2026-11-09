//! Export the resource manifest meta types as TypeScript definitions (via
//! specta) and JSON schemas (via schemars) into `packages/manifest-metadata`.

use std::borrow::Cow;
use std::collections::HashSet;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use manifest_core::{
    ChecksumKind, ChecksumMode, ChecksumPlan, DownloadOptions, DownloadPlan, FinishVersionError,
    RemoteVersionPlan, RequestPlan, ResolveError, ResolvedChecksum, RuntimeErrorShape,
    VariantChecksumKind, VariantSelection, VerifyError, VerifyPolicy, VersionOptions, VersionPlan,
};
use manifest_metadata::{
    FiniteF64, RESOURCE_MANIFEST_VERSION, ResourceManifest, ResourceManifestTemplate,
};
use manifest_wasm::{LimitsInput, RemoteResponseInput, RenderedArtifact, TemplateValue};
use serde_json::Value;
use specta::datatype::{DataType, Field, Fields, NamedReferenceType, Primitive, Reference};
use specta::{Format, FormatError, Type, Types};
use specta_typescript::Typescript;

/// Attribute key recorded by the specta derive for
/// `#[serde(skip_serializing_if = ...)]`, as defined by `specta-serde`.
const FIELD_SKIP_SERIALIZING_IF: &str = "serde:field:skip_serializing_if";
const MANIFEST_VERSION_TYPE: &str = "`2.${number}`";

/// TypeScript export shape with these adjustments over `specta_serde::Format`:
///
/// 1. Fields marked with `skip_serializing_if` are reported as optional
///    properties (`field?: T`) instead of being rejected. The unified format
///    refuses the attribute because a unified type graph cannot express
///    conditional omission, and `PhasesFormat` would duplicate every divergent
///    type as `*_Serialize` / `*_Deserialize`. The meta types only use the
///    attribute for optional JSON fields (and mark them `#[specta(optional)]`),
///    so stripping it up front keeps the export faithful.
/// 2. `manifest_version` is exported as the template literal `` `2.${number}` ``.
/// 3. `ResourceChecksum` is exported as the template literal
///    `` `${ChecksumAlgorithm}:${string}` `` instead of a plain string. A
///    `define` reference cannot serve as a named type body (the exporter
///    drops it), so this adjustment happens as an exact text replacement in
///    [`export_typescript`].
/// 4. specta-typescript renders any `f64` as `number | null` because serde_json
///    turns NaN/infinity into `null`. The only float in the graph is
///    `FiniteF64`, which rejects non-finite values by construction, so the
///    semantic lossless-float remap (`number`) is applied. The remap is
///    collection-level only (`apply_types`); `map_type` stays unmapped, which
///    is fine because the exporter only drives `map_types` here.
///    `validate_f64` checks this invariant before the remap.
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

    fn adjust(types: &mut Types) -> Result<(), FormatError> {
        let mut rewrites = 0;
        types.iter_mut(|named| {
            if let Some(ty) = named.ty.as_mut() {
                if matches!(
                    named.name.as_ref(),
                    "ResourceManifest" | "ResourceManifestTemplate"
                ) && let DataType::Struct(strct) = ty
                    && let Fields::Named(fields) = &mut strct.fields
                {
                    for (name, field) in &mut fields.fields {
                        if name == "manifest_version" {
                            field.ty = Some(DataType::Reference(specta_typescript::define(
                                MANIFEST_VERSION_TYPE,
                            )));
                            rewrites += 1;
                        }
                    }
                }
                Self::adjust_datatype(ty);
            }
        });
        if rewrites != 2 {
            return Err(
                format!("expected exactly 2 manifest_version rewrites, found {rewrites}").into(),
            );
        }
        Ok(())
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
                for (_, field) in &mut named.fields {
                    Self::adjust_field(field);
                }
            }
            Fields::Unnamed(unnamed) => {
                for field in &mut unnamed.fields {
                    Self::adjust_field(field);
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

    fn adjust_field(field: &mut Field) {
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
        validate_f64(types)?;
        let mut types = types.clone();
        Self::adjust(&mut types)?;
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

fn validate_f64(types: &Types) -> Result<(), FormatError> {
    let finite = FiniteF64::definition(&mut Types::default());
    for named in types.into_sorted_iter() {
        let count = named
            .ty
            .as_ref()
            .map(|ty| count_f64(ty, &finite))
            .unwrap_or_default()
            + named
                .generics
                .iter()
                .filter_map(|generic| generic.default.as_ref())
                .map(|ty| count_f64(ty, &finite))
                .sum::<usize>();
        if count != 0 {
            return Err(format!(
                "unexpected bare f64 in {}::{}; f64 is only allowed inside FiniteF64",
                named.module_path, named.name
            )
            .into());
        }
    }
    Ok(())
}

#[must_use]
fn count_f64(ty: &DataType, finite: &DataType) -> usize {
    if ty == finite {
        return 0;
    }
    match ty {
        DataType::Primitive(Primitive::f64) => 1,
        DataType::Primitive(_) | DataType::Generic(_) => 0,
        DataType::List(list) => count_f64(&list.ty, finite),
        DataType::Map(map) => count_f64(map.key_ty(), finite) + count_f64(map.value_ty(), finite),
        DataType::Struct(strct) => count_f64_fields(&strct.fields, finite),
        DataType::Enum(enm) => enm
            .variants
            .iter()
            .map(|(_, variant)| count_f64_fields(&variant.fields, finite))
            .sum(),
        DataType::Tuple(tuple) => tuple.elements.iter().map(|ty| count_f64(ty, finite)).sum(),
        DataType::Nullable(inner) => count_f64(inner, finite),
        DataType::Intersection(types) => types.iter().map(|ty| count_f64(ty, finite)).sum(),
        DataType::Reference(Reference::Named(reference)) => match &reference.inner {
            NamedReferenceType::Inline { dt, .. } => count_f64(dt, finite),
            NamedReferenceType::Reference { generics, .. } => {
                generics.iter().map(|(_, ty)| count_f64(ty, finite)).sum()
            }
            NamedReferenceType::Recursive(_) => 0,
        },
        DataType::Reference(Reference::Opaque(_)) => 0,
    }
}

#[must_use]
fn count_f64_fields(fields: &Fields, finite: &DataType) -> usize {
    match fields {
        Fields::Named(named) => named
            .fields
            .iter()
            .filter_map(|(_, field)| field.ty.as_ref())
            .map(|ty| count_f64(ty, finite))
            .sum(),
        Fields::Unnamed(unnamed) => unnamed
            .fields
            .iter()
            .filter_map(|field| field.ty.as_ref())
            .map(|ty| count_f64(ty, finite))
            .sum(),
        Fields::Unit => 0,
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
fn normalize_schema(schema: &mut schemars::Schema) -> Result<(), Box<dyn Error>> {
    if let Some(object) = schema.as_object_mut() {
        normalize_schema_map(object)?;
    }
    Ok(())
}

fn normalize_schema_map(object: &mut serde_json::Map<String, Value>) -> Result<(), Box<dyn Error>> {
    if let Some(format) = object.get("format") {
        let numeric = format.as_str().is_some_and(|format| {
            matches!(format, "float" | "double")
                || format
                    .strip_prefix("uint")
                    .or_else(|| format.strip_prefix("int"))
                    .is_some_and(|width| width.bytes().all(|byte| byte.is_ascii_digit()))
        });
        if !numeric {
            return Err(format!(
                "unexpected schema format {format}; only integer and float formats may be removed"
            )
            .into());
        }
        object.shift_remove("format");
    }

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

    for (keyword, child) in object.iter_mut() {
        match keyword.as_str() {
            "properties" | "patternProperties" | "$defs" | "definitions" | "dependentSchemas"
            | "dependencies" => {
                if let Value::Object(schemas) = child {
                    for schema in schemas.values_mut() {
                        if let Value::Object(object) = schema {
                            normalize_schema_map(object)?;
                        }
                    }
                }
            }
            "allOf"
            | "anyOf"
            | "oneOf"
            | "prefixItems"
            | "items"
            | "not"
            | "if"
            | "then"
            | "else"
            | "contains"
            | "additionalProperties"
            | "unevaluatedProperties"
            | "unevaluatedItems"
            | "propertyNames"
            | "additionalItems"
            | "contentSchema" => {
                normalize_schema_value(child)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn normalize_schema_value(value: &mut Value) -> Result<(), Box<dyn Error>> {
    match value {
        Value::Object(object) => normalize_schema_map(object)?,
        Value::Array(items) => {
            for item in items {
                normalize_schema_value(item)?;
            }
        }
        _ => {}
    }
    Ok(())
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
    let bindings = bindings.replace(CHECKSUM_ALIAS, CHECKSUM_TEMPLATE);

    // The package contract is "absent or concrete value" (see the metadata
    // README), so no optional property may render as nullable.
    if bindings.contains("| null") {
        return Err(format!(
            "generated TypeScript in {} contains `| null`; optional fields must \
             stay non-nullable (see the SerdeShape lossless-float note)",
            out.display()
        )
        .into());
    }

    fs::write(&out, bindings)?;
    Ok(out)
}

fn export_version(out_dir: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let out = out_dir.join("src/version.ts");
    let version = serde_json::to_string(&RESOURCE_MANIFEST_VERSION)?;
    fs::write(
        &out,
        format!(
            "// This file has been generated by manifest-schema-generator. Do not edit this file manually.\n\n\
             export const RESOURCE_MANIFEST_VERSION = {version};\n"
        ),
    )?;
    Ok(out)
}

/// Runtime types are owned by core or by the wasm boundary; everything else is
/// imported from `@manifest/metadata`.
fn is_runtime_type(module_path: &str) -> bool {
    module_path.starts_with("manifest_core") || module_path.starts_with("manifest_wasm")
}

fn export_runtime_typescript() -> Result<PathBuf, Box<dyn Error>> {
    let mut types = Types::default()
        .register::<ChecksumKind>()
        .register::<ChecksumMode>()
        .register::<ChecksumPlan>()
        .register::<DownloadOptions<'_>>()
        .register::<DownloadPlan>()
        .register::<FinishVersionError>()
        .register::<RemoteVersionPlan>()
        .register::<RequestPlan>()
        .register::<ResolveError>()
        .register::<ResolvedChecksum>()
        .register::<RuntimeErrorShape<specta_typescript::Unknown>>()
        .register::<VariantChecksumKind>()
        .register::<VariantSelection<'_>>()
        .register::<VerifyError>()
        .register::<VerifyPolicy>()
        .register::<VersionOptions<'_>>()
        .register::<VersionPlan>()
        .register::<LimitsInput>()
        .register::<RemoteResponseInput>()
        .register::<RenderedArtifact>()
        .register::<TemplateValue>();
    types.iter_mut(|named| {
        if let Some(ty) = named.ty.as_mut() {
            SerdeShape::adjust_datatype(ty);
        }
    });
    let types = specta_serde::Format
        .map_types(&types)
        .map_err(|error| -> Box<dyn Error> { error })?
        .into_owned();
    let (bindings, references) = specta_typescript::collect_references(|| {
        specta_typescript::primitives::export(
            &Typescript::default(),
            &types,
            types
                .into_sorted_iter()
                .filter(|named| is_runtime_type(&named.module_path)),
            "",
        )
    });
    let bindings = bindings?;
    let mut imports: Vec<_> = references
        .iter()
        .filter_map(|reference| types.get(reference))
        .filter(|named| !is_runtime_type(&named.module_path))
        .map(|named| named.name.as_ref())
        .collect();
    imports.sort_unstable();
    imports.dedup();
    let out =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/manifest-runtime/src/types.ts");
    fs::create_dir_all(
        out.parent()
            .ok_or("runtime types need a parent directory")?,
    )?;
    fs::write(
        &out,
        format!(
            "// This file has been generated by manifest-schema-generator. Do not edit this file manually.\n\nimport type {{ {} }} from \"@manifest/metadata\";\n\n{bindings}",
            imports.join(", ")
        ),
    )?;
    Ok(out)
}

/// The canonical checksum pattern, matching `ResourceChecksum`'s schemars
/// impl. Used for the compiled manifest schema.
const CANONICAL_CHECKSUM_PATTERN: &str =
    r"^(md5:[a-f0-9]{32}|sha256:[a-f0-9]{64}|sha512:[a-f0-9]{128})$";

/// The template schema accepts anything the compiler can normalize, mirroring
/// `ResourceChecksum::new` / the reference `parseResourceChecksum`.
const RELAXED_CHECKSUM_PATTERN: &str = r"^([mM][dD]5:[a-fA-F0-9]{32}|[sS][hH][aA]256:[a-fA-F0-9]{64}|[sS][hH][aA]512:[a-fA-F0-9]{128})$";

/// Authored templates may use any casing in checksums; the compiler
/// normalizes to lowercase. The compiled manifest schema keeps the canonical
/// pattern. Fails loudly if the canonical pattern is not where it is
/// expected, so a changed schemars output cannot drift silently.
fn relax_checksum_pattern(schema: &mut schemars::Schema) -> Result<(), Box<dyn Error>> {
    let pattern = schema
        .as_object_mut()
        .and_then(|object| object.get_mut("$defs"))
        .and_then(Value::as_object_mut)
        .and_then(|defs| defs.get_mut("ResourceChecksum"))
        .and_then(Value::as_object_mut)
        .and_then(|checksum| checksum.get_mut("pattern"));

    match pattern {
        Some(Value::String(pattern)) if *pattern == CANONICAL_CHECKSUM_PATTERN => {
            *pattern = RELAXED_CHECKSUM_PATTERN.to_string();
            Ok(())
        }
        _ => Err("ResourceChecksum canonical pattern not found in template schema".into()),
    }
}

fn export_json_schema<T: schemars::JsonSchema>(
    out_dir: &Path,
    file_name: &str,
    authored_template: bool,
) -> Result<PathBuf, Box<dyn Error>> {
    let settings = schemars::generate::SchemaSettings::draft2020_12();
    let mut schema = settings.into_generator().into_root_schema_for::<T>();
    schema.insert(
        "$id".into(),
        Value::String(format!(
            "https://libnyanpasu.github.io/resources-manifest/schema/{RESOURCE_MANIFEST_VERSION}/{file_name}"
        )),
    );
    normalize_schema(&mut schema)?;
    if authored_template {
        relax_checksum_pattern(&mut schema)?;
    }
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
        export_version(&out_dir)?,
        export_runtime_typescript()?,
        export_json_schema::<ResourceManifest>(&out_dir, "manifest.schema.json", false)?,
        export_json_schema::<ResourceManifestTemplate>(
            &out_dir,
            "manifest-template.schema.json",
            true,
        )?,
    ];

    for path in written {
        println!("wrote {}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use specta::datatype::{NamedDataType, Primitive, Struct};

    #[test]
    fn manifest_version_rewrites_only_the_two_root_types() {
        let mut types = Types::default()
            .register::<ResourceManifest>()
            .register::<ResourceManifestTemplate>();
        let unrelated = Struct::named()
            .field("manifest_version", Field::new(Primitive::str.into()))
            .build();
        NamedDataType::new("Unrelated", &mut types, |_, named| {
            named.ty = Some(unrelated.clone());
        });
        SerdeShape::adjust(&mut types).expect("rewrite the two manifest roots");
        for named in types.into_unsorted_iter() {
            if named.name == "Unrelated" {
                assert_eq!(named.ty.as_ref(), Some(&unrelated));
            } else if matches!(
                named.name.as_ref(),
                "ResourceManifest" | "ResourceManifestTemplate"
            ) {
                let DataType::Struct(strct) = named.ty.as_ref().expect("root type exists") else {
                    panic!("manifest root must be a struct");
                };
                let Fields::Named(fields) = &strct.fields else {
                    panic!("manifest root must have named fields");
                };
                let (_, field) = fields
                    .fields
                    .iter()
                    .find(|(name, _)| name == "manifest_version")
                    .expect("manifest root has a version field");
                assert_eq!(
                    field.ty,
                    Some(DataType::Reference(specta_typescript::define(
                        MANIFEST_VERSION_TYPE
                    )))
                );
            }
        }
        for mut incomplete in [
            Types::default(),
            Types::default().register::<ResourceManifest>(),
        ] {
            let error = SerdeShape::adjust(&mut incomplete).expect_err("missing roots must fail");
            assert!(
                error
                    .to_string()
                    .contains("expected exactly 2 manifest_version rewrites")
            );
        }
        NamedDataType::new("ResourceManifest", &mut types, |_, named| {
            named.ty = Some(unrelated);
        });
        assert_eq!(
            SerdeShape::adjust(&mut types)
                .expect_err("extra root must fail")
                .to_string(),
            "expected exactly 2 manifest_version rewrites, found 3"
        );
    }

    #[test]
    fn schema_normalization_only_removes_numeric_formats_on_schema_nodes() {
        let mut schema = schemars::json_schema!({
            "type": "object",
            "properties": {
                "format": { "type": "integer", "format": "uint32" },
                "optional": { "type": ["number", "null"], "format": "double" }
            },
            "required": ["format"],
            "$defs": { "format": { "type": "number", "format": "float" } },
            "examples": [{ "format": "date-time" }],
            "default": { "format": "email" },
            "const": { "format": "uuid" }
        });
        normalize_schema(&mut schema).expect("normalize numeric schema formats");
        assert_eq!(
            schema,
            schemars::json_schema!({
                "type": "object",
                "properties": {
                    "format": { "type": "integer" },
                    "optional": { "type": "number" }
                },
                "required": ["format"],
                "$defs": { "format": { "type": "number" } },
                "examples": [{ "format": "date-time" }],
                "default": { "format": "email" },
                "const": { "format": "uuid" }
            })
        );
        for format in [
            "uint", "uint8", "uint32", "uint128", "int", "int64", "float", "double",
        ] {
            let mut schema = schemars::json_schema!({ "format": format });
            normalize_schema(&mut schema).expect("numeric format is supported");
            assert_eq!(schema, schemars::json_schema!({}), "format {format}");
        }
        for format in [
            serde_json::json!("date-time"),
            serde_json::json!("email"),
            serde_json::json!("integerish"),
            serde_json::json!(null),
            serde_json::json!(42),
        ] {
            let mut schema = schemars::json_schema!({ "format": format });
            let error = normalize_schema(&mut schema).expect_err("unexpected format must fail");
            assert!(error.to_string().contains("unexpected schema format"));
        }
        for keyword in [
            "properties",
            "patternProperties",
            "$defs",
            "definitions",
            "dependentSchemas",
            "dependencies",
        ] {
            let mut value = serde_json::json!({});
            value[keyword] = serde_json::json!({ "format": { "format": "date-time" } });
            let mut schema = schemars::Schema::try_from(value).expect("schema object");
            assert!(normalize_schema(&mut schema).is_err(), "keyword {keyword}");
        }
        for keyword in [
            "allOf",
            "anyOf",
            "oneOf",
            "prefixItems",
            "items",
            "not",
            "if",
            "then",
            "else",
            "contains",
            "additionalProperties",
            "unevaluatedProperties",
            "unevaluatedItems",
            "propertyNames",
            "additionalItems",
            "contentSchema",
        ] {
            let mut value = serde_json::json!({});
            value[keyword] = if matches!(keyword, "allOf" | "anyOf" | "oneOf" | "prefixItems") {
                serde_json::json!([{ "format": "date-time" }])
            } else {
                serde_json::json!({ "format": "date-time" })
            };
            let mut schema = schemars::Schema::try_from(value).expect("schema object");
            assert!(normalize_schema(&mut schema).is_err(), "keyword {keyword}");
        }
    }

    #[test]
    fn f64_is_only_allowed_inside_finite_f64_before_remapping() {
        use specta::datatype::{Enum, Generic, GenericDefinition, List, Map, Tuple, Variant};

        let mut types = Types::default()
            .register::<ResourceManifest>()
            .register::<ResourceManifestTemplate>();
        let shape = SerdeShape::new();
        shape
            .map_types(&types)
            .expect("current graph only uses FiniteF64");
        validate_f64(&Types::default()).expect("a graph without f64 is valid");
        NamedDataType::new("ExtraFinite", &mut types, |types, named| {
            named.ty = Some(
                Struct::named()
                    .field("value", Field::new(FiniteF64::definition(types)))
                    .field(
                        "optional",
                        Field::new(<Option<FiniteF64>>::definition(types)),
                    )
                    .field("list", Field::new(<Vec<FiniteF64>>::definition(types)))
                    .build(),
            );
        });
        let bindings = Typescript::default()
            .export(&types, SerdeShape::new())
            .expect("additional FiniteF64 usage sites are valid");
        assert!(bindings.contains("value: number"), "{bindings}");

        let float = DataType::Primitive(Primitive::f64);
        let mut enm = Enum::default();
        enm.variants.push((
            "Value".into(),
            Variant::unnamed().field(Field::new(float.clone())).build(),
        ));
        let generic = NamedDataType::new("GenericContainer", &mut types, |_, named| {
            named.ty = Some(Primitive::str.into());
        })
        .reference(vec![(Generic::new("T".into()), float.clone())]);
        for ty in [
            float.clone(),
            DataType::Nullable(Box::new(List::new(float.clone()).into())),
            Map::new(Primitive::str.into(), float.clone()).into(),
            Map::new(float.clone(), Primitive::str.into()).into(),
            Struct::named()
                .field("value", Field::new(float.clone()))
                .build(),
            Struct::unnamed().field(Field::new(float.clone())).build(),
            enm.into(),
            Tuple::new(vec![float.clone()]).into(),
            DataType::Intersection(vec![float.clone()]),
            DataType::Reference(generic),
        ] {
            let mut invalid = types.clone();
            NamedDataType::new("Unexpected", &mut invalid, |_, named| {
                named.ty = Some(ty);
            });
            let error = shape
                .map_types(&invalid)
                .expect_err("extra f64 must fail before lossless remapping");
            assert!(error.to_string().contains("unexpected bare f64"), "{error}");
        }
        let mut impostor = types.clone();
        NamedDataType::new("FiniteF64", &mut impostor, |_, named| {
            named.module_path = "manifest_metadata".into();
            named.ty = Some(float.clone());
        });
        assert!(
            validate_f64(&impostor).is_err(),
            "the boundary must be identified by type identity"
        );
        let mut invalid = types;
        NamedDataType::new("DefaultFloat", &mut invalid, |_, named| {
            named.generics = vec![GenericDefinition::new("T".into(), Some(float))].into();
        });
        assert!(
            validate_f64(&invalid).is_err(),
            "generic defaults must be checked"
        );
    }
}
