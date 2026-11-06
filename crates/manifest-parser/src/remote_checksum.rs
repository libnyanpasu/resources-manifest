use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use specta::Type;

use crate::version::deserialize_parser_object;
use crate::{ChecksumAlgorithm, Limits, ParseError, RemoteResponse, ResourceChecksum};

/// How to parse a remote checksum response.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema, Type)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum RemoteChecksumParser {
    /// The response body itself is `<algorithm>:<hex>`.
    Checksum,
    /// GNU checksum list: `<hex>  <artifact>`.
    Gnu { algorithm: ChecksumAlgorithm },
    /// BSD checksum list: `SHA512 (<artifact>) = <hex>`.
    Bsd { algorithm: ChecksumAlgorithm },
    /// GitHub `/releases/expanded_assets/<tag>` HTML with asset digests.
    GithubReleaseAssets,
    #[schemars(skip, !untagged)]
    #[specta(skip)]
    #[serde(untagged)]
    Unsupported {
        #[serde(rename = "type")]
        parser_type: String,
    },
}

impl RemoteChecksumParser {
    #[must_use = "checksum parsing errors must be handled"]
    pub fn parse(
        &self,
        response: &RemoteResponse<'_>,
        artifact: &str,
        limits: &Limits,
    ) -> Result<ResourceChecksum, ParseError> {
        if response.body.len() > limits.body_bytes() {
            return Err(ParseError::ValidateBodySize {
                limit: limits.body_bytes(),
                actual: response.body.len(),
            });
        }
        match self {
            Self::Checksum => response
                .body
                .trim()
                .parse()
                .map_err(|_| ParseError::ValidateChecksum {}),
            Self::Gnu { algorithm } => parse_list(response.body, artifact, *algorithm, |line| {
                gnu_digest(line, artifact)
            }),
            Self::Bsd { algorithm } => parse_list(response.body, artifact, *algorithm, |line| {
                bsd_digest(line, artifact, *algorithm)
            }),
            Self::GithubReleaseAssets => parse_github_assets(response.body, artifact),
            Self::Unsupported { parser_type } => Err(ParseError::ResolveParser {
                parser_type: parser_type.clone(),
            }),
        }
    }
}

fn parse_github_assets(body: &str, artifact: &str) -> Result<ResourceChecksum, ParseError> {
    let dom = tl::parse(body, tl::ParserOptions::default())
        .map_err(|_| ParseError::ValidateChecksum {})?;
    let parser = dom.parser();
    let mut selected = None;
    for row in dom.nodes().iter().filter_map(tl::Node::as_tag) {
        if !row.name().as_utf8_str().eq_ignore_ascii_case("li") {
            continue;
        }
        let Some(digests) = github_entry_digests(row, parser, artifact) else {
            continue;
        };
        if digests.is_empty() {
            return Err(ParseError::ReadDigest {
                name: artifact.to_owned(),
            });
        }
        for digest in digests {
            let checksum =
                ResourceChecksum::new(digest).map_err(|_| ParseError::ValidateChecksum {})?;
            if let Some(previous) = &selected
                && previous != &checksum
            {
                return Err(ParseError::SelectChecksum {
                    name: artifact.to_owned(),
                });
            }
            selected = Some(checksum);
        }
    }
    selected.ok_or_else(|| ParseError::FindChecksum {
        name: artifact.to_owned(),
    })
}

fn github_entry_digests(
    row: &tl::HTMLTag<'_>,
    parser: &tl::Parser<'_>,
    artifact: &str,
) -> Option<Vec<String>> {
    let mut pending: Vec<_> = row
        .children()
        .top()
        .as_slice()
        .iter()
        .rev()
        .copied()
        .collect();
    let mut matched = false;
    let mut digests = Vec::new();
    while let Some(handle) = pending.pop() {
        let Some(tag) = handle.get(parser).and_then(tl::Node::as_tag) else {
            continue;
        };
        let name = tag.name().as_utf8_str();
        if ["li", "script", "style"]
            .iter()
            .any(|excluded| name.eq_ignore_ascii_case(excluded))
        {
            continue;
        }
        if name.eq_ignore_ascii_case("a")
            && let Some(href) = tag.attributes().get("href").flatten()
        {
            matched |= github_asset_matches(&href.as_utf8_str(), artifact);
        }
        if tag
            .attributes()
            .get("class")
            .flatten()
            .is_some_and(|classes| {
                classes
                    .as_utf8_str()
                    .split_ascii_whitespace()
                    .any(|class| class == "Truncate-text")
            })
        {
            digests.push(tag.inner_text(parser).trim().to_owned());
            continue;
        }
        pending.extend(tag.children().top().as_slice().iter().rev().copied());
    }
    matched.then_some(digests)
}

fn github_asset_matches(href: &str, artifact: &str) -> bool {
    let Some((_, path)) = href.split_once("/releases/download/") else {
        return false;
    };
    let path = path.split(['?', '#']).next().unwrap_or_default();
    let Some((_, name)) = path.rsplit_once('/') else {
        return false;
    };
    !name.is_empty()
        && percent_encoding::percent_decode_str(name)
            .decode_utf8()
            .is_ok_and(|name| name == artifact)
}

fn parse_list<'a>(
    body: &'a str,
    artifact: &str,
    algorithm: ChecksumAlgorithm,
    digest_for_line: impl Fn(&'a str) -> Option<&'a str>,
) -> Result<ResourceChecksum, ParseError> {
    let mut selected = None;
    for line in body.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let Some(digest) = digest_for_line(line) else {
            continue;
        };
        if digest.len() != algorithm.hex_len()
            || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            continue;
        }
        let checksum = ResourceChecksum::new(format!("{}:{digest}", algorithm.as_str()))
            .map_err(|_| ParseError::ValidateChecksum {})?;
        if let Some(previous) = &selected
            && previous != &checksum
        {
            return Err(ParseError::SelectChecksum {
                name: artifact.to_owned(),
            });
        }
        selected = Some(checksum);
    }
    selected.ok_or_else(|| ParseError::FindChecksum {
        name: artifact.to_owned(),
    })
}

fn gnu_digest<'a>(line: &'a str, artifact: &str) -> Option<&'a str> {
    let (digest, rest) = line.split_once([' ', '\t'])?;
    let name = rest.trim_end_matches([' ', '\t']);
    let requested = artifact.trim_end_matches([' ', '\t']);
    if rest.len() - name.len() < artifact.len() - requested.len() {
        return None;
    }
    let prefix = name.strip_suffix(requested)?;
    let separator = prefix.strip_suffix('*').unwrap_or(prefix);
    separator
        .bytes()
        .all(|byte| matches!(byte, b' ' | b'\t'))
        .then_some(digest)
}

fn bsd_digest<'a>(line: &'a str, artifact: &str, algorithm: ChecksumAlgorithm) -> Option<&'a str> {
    let (label, rest) = line.split_once(" (")?;
    if !label.eq_ignore_ascii_case(algorithm.as_str()) {
        return None;
    }
    let (name, digest) = rest.rsplit_once(") = ")?;
    (name == artifact).then_some(digest)
}

impl<'de> Deserialize<'de> for RemoteChecksumParser {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "kebab-case")]
        enum Known {
            Checksum,
            Gnu { algorithm: ChecksumAlgorithm },
            Bsd { algorithm: ChecksumAlgorithm },
            GithubReleaseAssets,
        }

        let value = deserialize_parser_object(deserializer)?;
        let parser_type = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| serde::de::Error::custom("parser type must be a string"))?;
        if !matches!(
            parser_type,
            "checksum" | "gnu" | "bsd" | "github-release-assets"
        ) {
            return Ok(Self::Unsupported {
                parser_type: parser_type.to_owned(),
            });
        }
        match Known::deserialize(value).map_err(serde::de::Error::custom)? {
            Known::Checksum => Ok(Self::Checksum),
            Known::Gnu { algorithm } => Ok(Self::Gnu { algorithm }),
            Known::Bsd { algorithm } => Ok(Self::Bsd { algorithm }),
            Known::GithubReleaseAssets => Ok(Self::GithubReleaseAssets),
        }
    }
}
