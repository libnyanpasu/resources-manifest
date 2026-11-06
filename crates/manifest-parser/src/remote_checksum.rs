use base64::{Engine, engine::general_purpose::STANDARD};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use specta::Type;

use crate::version::deserialize_parser_object;
use crate::{ChecksumAlgorithm, Limits, ParseError, RemoteResponse, ResourceChecksum};

/// How a `header` checksum parser decodes the header value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "kebab-case")]
pub enum HeaderEncoding {
    /// Hexadecimal digest, any case.
    Hex,
    /// Standard base64 (`+/`, padded) of the raw digest bytes.
    Base64,
}

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
    /// The digest is read from the named response header (case-insensitive).
    Header {
        name: String,
        algorithm: ChecksumAlgorithm,
        encoding: HeaderEncoding,
    },
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
        if !matches!(self, Self::Header { .. }) && response.body.len() > limits.body_bytes() {
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
            Self::Header {
                name,
                algorithm,
                encoding,
            } => parse_header(response, name, *algorithm, *encoding, limits),
            Self::Unsupported { parser_type } => Err(ParseError::ResolveParser {
                parser_type: parser_type.clone(),
            }),
        }
    }
}

fn parse_header(
    response: &RemoteResponse<'_>,
    name: &str,
    algorithm: ChecksumAlgorithm,
    encoding: HeaderEncoding,
    limits: &Limits,
) -> Result<ResourceChecksum, ParseError> {
    for value in response.header_values(name) {
        if value.len() > limits.header_value_bytes() {
            return Err(ParseError::ValidateHeaderSize {
                name: name.to_owned(),
                limit: limits.header_value_bytes(),
                actual: value.len(),
            });
        }
    }
    let value = response.single_header(name)?;
    let digest = match encoding {
        HeaderEncoding::Hex => value.to_owned(),
        HeaderEncoding::Base64 => STANDARD
            .decode(value)
            .map_err(|source| ParseError::DecodeHeader {
                name: name.to_owned(),
                source,
            })?
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    };
    ResourceChecksum::new(format!("{}:{digest}", algorithm.as_str())).map_err(|_| {
        ParseError::ValidateHeaderChecksum {
            name: name.to_owned(),
        }
    })
}

fn parse_github_assets(body: &str, artifact: &str) -> Result<ResourceChecksum, ParseError> {
    let dom = tl::parse(body, tl::ParserOptions::default())
        .map_err(|_| ParseError::ValidateChecksum {})?;
    let parser = dom.parser();
    let mut selected = None;
    let mut pending: Vec<_> = dom.children().iter().rev().copied().collect();
    while let Some(handle) = pending.pop() {
        let Some(row) = handle.get(parser).and_then(tl::Node::as_tag) else {
            continue;
        };
        let name = row.name().as_utf8_str();
        if ["script", "style"]
            .iter()
            .any(|excluded| name.eq_ignore_ascii_case(excluded))
        {
            continue;
        }
        pending.extend(row.children().top().as_slice().iter().rev().copied());
        if !name.eq_ignore_ascii_case("li") {
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
            if let Some(digest) = github_digest_text(tag, parser) {
                digests.push(digest);
            }
            continue;
        }
        pending.extend(tag.children().top().as_slice().iter().rev().copied());
    }
    matched.then_some(digests)
}

fn github_digest_text(tag: &tl::HTMLTag<'_>, parser: &tl::Parser<'_>) -> Option<String> {
    let mut pending: Vec<_> = tag
        .children()
        .top()
        .as_slice()
        .iter()
        .rev()
        .copied()
        .collect();
    let mut text = String::new();
    let mut excluded = false;
    while let Some(handle) = pending.pop() {
        match handle.get(parser)? {
            tl::Node::Raw(raw) => text.push_str(&raw.as_utf8_str()),
            tl::Node::Comment(_) => excluded = true,
            tl::Node::Tag(tag) => {
                let name = tag.name().as_utf8_str();
                if ["li", "script", "style"]
                    .iter()
                    .any(|excluded| name.eq_ignore_ascii_case(excluded))
                {
                    excluded = true;
                    continue;
                }
                pending.extend(tag.children().top().as_slice().iter().rev().copied());
            }
        }
    }
    let text = text.trim();
    (!excluded || !text.is_empty()).then(|| text.to_owned())
}

fn github_asset_matches(href: &str, artifact: &str) -> bool {
    let href = decode_html_attribute(href);
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

fn decode_html_attribute(value: &str) -> String {
    let mut parts = value.split('&');
    let mut decoded = String::with_capacity(value.len());
    decoded.push_str(parts.next().unwrap_or_default());
    for part in parts {
        if let Some((reference, rest)) = part.split_once(';')
            && let Some(character) = html_reference(reference)
        {
            decoded.push(character);
            decoded.push_str(rest);
        } else {
            decoded.push('&');
            decoded.push_str(part);
        }
    }
    decoded
}

fn html_reference(reference: &str) -> Option<char> {
    match reference {
        "amp" | "AMP" => Some('&'),
        "quot" | "QUOT" => Some('"'),
        "apos" => Some('\''),
        "lt" | "LT" => Some('<'),
        "gt" | "GT" => Some('>'),
        "nbsp" => Some('\u{a0}'),
        _ => {
            let number = reference.strip_prefix('#')?;
            let (digits, radix) = number
                .strip_prefix(['x', 'X'])
                .map_or((number, 10), |digits| (digits, 16));
            if digits.is_empty()
                || !digits.bytes().all(|byte| match radix {
                    16 => byte.is_ascii_hexdigit(),
                    _ => byte.is_ascii_digit(),
                })
            {
                return None;
            }
            let character = u32::from_str_radix(digits, radix)
                .ok()
                .filter(|&value| value != 0)
                .and_then(char::from_u32)
                .unwrap_or(char::REPLACEMENT_CHARACTER);
            Some(character)
        }
    }
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
    rest.rmatch_indices(artifact)
        .any(|(start, name)| {
            let prefix = &rest[..start];
            let separator = prefix.strip_suffix(['*', ' ']).unwrap_or(prefix);
            separator.bytes().all(|byte| matches!(byte, b' ' | b'\t'))
                && rest[start + name.len()..]
                    .bytes()
                    .all(|byte| matches!(byte, b' ' | b'\t'))
        })
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
            Gnu {
                algorithm: ChecksumAlgorithm,
            },
            Bsd {
                algorithm: ChecksumAlgorithm,
            },
            GithubReleaseAssets,
            Header {
                name: String,
                algorithm: ChecksumAlgorithm,
                encoding: HeaderEncoding,
            },
        }

        let value = deserialize_parser_object(deserializer)?;
        let parser_type = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| serde::de::Error::custom("parser type must be a string"))?;
        if !matches!(
            parser_type,
            "checksum" | "gnu" | "bsd" | "github-release-assets" | "header"
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
            Known::Header {
                name,
                algorithm,
                encoding,
            } => Ok(Self::Header {
                name,
                algorithm,
                encoding,
            }),
        }
    }
}
