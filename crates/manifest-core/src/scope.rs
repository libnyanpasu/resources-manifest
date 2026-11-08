use std::collections::BTreeMap;

use manifest_metadata::{MirrorDefinition, ResourceManifest, SourceVariables};

#[must_use]
pub(crate) fn mirror_allows(mirror: &MirrorDefinition, registry: &str) -> bool {
    mirror
        .registries
        .as_ref()
        .is_none_or(|allowed| allowed.is_empty() || allowed.iter().any(|id| id == registry))
}

pub(crate) type Scope<'a, V = &'a str> = BTreeMap<&'a str, (V, String)>;

pub(crate) const BUILTINS: &[&str] = &[
    "resource",
    "resource_id",
    "variant",
    "variant_id",
    "tag_key",
    "platform",
    "version",
];
pub(crate) const URL_COMPONENTS: &[&str] = &[
    "url",
    "url_encoded",
    "scheme",
    "host",
    "hostname",
    "port",
    "path",
    "pathname",
    "query",
    "path_and_query",
];

pub(crate) fn merge<'a, V: From<&'a str>>(
    scope: &mut Scope<'a, V>,
    source: Option<&'a SourceVariables>,
    path: &str,
) {
    if let Some(source) = source {
        for (key, value) in source {
            scope.insert(key, (V::from(value.as_str()), format!("{path}.{key}")));
        }
    }
}

#[must_use]
pub(crate) fn registry_scope<'a, V: From<&'a str>>(
    manifest: &'a ResourceManifest,
    registry: &str,
) -> Scope<'a, V> {
    let mut scope = Scope::new();
    if let Some(definition) = manifest.registries.get(registry) {
        merge(
            &mut scope,
            definition.variables.as_ref(),
            &format!("registries.{registry}.variables"),
        );
    }
    scope
}

#[must_use = "template variable references must be inspected"]
pub(crate) fn variables(template: &str) -> impl Iterator<Item = &str> {
    let mut bytes = template.bytes().enumerate();
    let mut start = None;
    std::iter::from_fn(move || {
        for (index, byte) in bytes.by_ref() {
            match byte {
                b'{' => start = Some(index + 1),
                b'}' => {
                    if let Some(start) = start.take().filter(|start| *start < index) {
                        return Some(&template[start..index]);
                    }
                }
                byte if byte.is_ascii_alphanumeric() || byte == b'_' => {}
                _ => start = None,
            }
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use super::variables;

    #[test]
    fn variable_syntax() {
        for (template, expected) in [
            ("{{a}}", vec!["a"]),
            ("{a{b}", vec!["b"]),
            ("{}", vec![]),
            ("{a-b}", vec![]),
            ("{a_b09}{Z}{0}", vec!["a_b09", "Z", "0"]),
            ("{é}{a b}{x.y}{bad-{good}}", vec!["good"]),
            ("{unclosed", vec![]),
        ] {
            assert_eq!(
                variables(template).collect::<Vec<_>>(),
                expected,
                "{template}"
            );
        }
    }

    #[test]
    fn long_unclosed_brace_run() {
        let mut template = "{".repeat(1024 * 1024);
        assert_eq!(variables(&template).next(), None);
        template.push_str("tail}");
        assert_eq!(variables(&template).collect::<Vec<_>>(), ["tail"]);
    }
}
