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
    let mut values = response
        .header_values(name)
        .flat_map(|value| value.split(','))
        .map(|value| value.trim_matches([' ', '\t']))
        .filter(|value| !value.is_empty());
    let value = values.next().ok_or_else(|| ParseError::FindHeader {
        name: name.to_owned(),
    })?;
    if values.any(|other| other != value) {
        return Err(ParseError::SelectHeader {
            name: name.to_owned(),
        });
    }
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
    let path = href.split(['?', '#']).next().unwrap_or_default();
    let Some((_, path)) = path.split_once("/releases/download/") else {
        return false;
    };
    let Some((_, name)) = path.rsplit_once('/') else {
        return false;
    };
    !name.is_empty()
        && percent_encoding::percent_decode_str(name)
            .decode_utf8()
            .is_ok_and(|name| name == artifact)
}

fn decode_html_attribute(value: &str) -> String {
    let mut decoded = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find('&') {
        decoded.push_str(&rest[..start]);
        rest = &rest[start + 1..];
        if let Some((character, consumed)) = html_reference(rest) {
            decoded.push(character);
            rest = &rest[consumed..];
        } else {
            decoded.push('&');
        }
    }
    decoded.push_str(rest);
    decoded
}

fn html_reference(reference: &str) -> Option<(char, usize)> {
    if let Some(number) = reference.strip_prefix('#') {
        let (digits, radix) = number
            .strip_prefix(['x', 'X'])
            .map_or((number, 10), |digits| (digits, 16));
        let mut value = 0_u32;
        let mut length = 0;
        for byte in digits.bytes() {
            let Some(digit) = char::from(byte).to_digit(radix) else {
                break;
            };
            value = value.saturating_mul(radix).saturating_add(digit);
            length += 1;
        }
        if length == 0 {
            return None;
        }
        let consumed = reference.len() - digits.len()
            + length
            + usize::from(digits[length..].starts_with(';'));
        return Some((numeric_html_character(value), consumed));
    }
    for (name, character) in [
        ("amp", '&'),
        ("AMP", '&'),
        ("lt", '<'),
        ("LT", '<'),
        ("gt", '>'),
        ("GT", '>'),
        ("quot", '"'),
        ("QUOT", '"'),
        ("apos", '\''),
        ("nbsp", '\u{a0}'),
        ("num", '#'),
        ("sol", '/'),
        ("colon", ':'),
        ("quest", '?'),
        ("equals", '='),
        ("period", '.'),
        ("plus", '+'),
        ("percnt", '%'),
        ("excl", '!'),
        ("commat", '@'),
        ("lpar", '('),
        ("rpar", ')'),
        ("lsqb", '['),
        ("rsqb", ']'),
        ("lowbar", '_'),
        ("Hat", '^'),
        ("grave", '`'),
        ("verbar", '|'),
        ("tilde", '~'),
    ] {
        let Some(rest) = reference.strip_prefix(name) else {
            continue;
        };
        if rest.starts_with(';') {
            return Some((character, name.len() + 1));
        }
        if matches!(
            name,
            "amp" | "AMP" | "lt" | "LT" | "gt" | "GT" | "quot" | "QUOT"
        ) && !rest
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'=')
        {
            return Some((character, name.len()));
        }
    }
    None
}

fn numeric_html_character(value: u32) -> char {
    let value = match value {
        0 => return char::REPLACEMENT_CHARACTER,
        0x80 => 0x20ac,
        0x82 => 0x201a,
        0x83 => 0x0192,
        0x84 => 0x201e,
        0x85 => 0x2026,
        0x86 => 0x2020,
        0x87 => 0x2021,
        0x88 => 0x02c6,
        0x89 => 0x2030,
        0x8a => 0x0160,
        0x8b => 0x2039,
        0x8c => 0x0152,
        0x8e => 0x017d,
        0x91 => 0x2018,
        0x92 => 0x2019,
        0x93 => 0x201c,
        0x94 => 0x201d,
        0x95 => 0x2022,
        0x96 => 0x2013,
        0x97 => 0x2014,
        0x98 => 0x02dc,
        0x99 => 0x2122,
        0x9a => 0x0161,
        0x9b => 0x203a,
        0x9c => 0x0153,
        0x9e => 0x017e,
        0x9f => 0x0178,
        _ => value,
    };
    char::from_u32(value).unwrap_or(char::REPLACEMENT_CHARACTER)
}

fn parse_list<'a>(
    body: &'a str,
    artifact: &str,
    algorithm: ChecksumAlgorithm,
    digest_for_line: impl Fn(&'a str) -> Option<&'a str>,
) -> Result<ResourceChecksum, ParseError> {
    if artifact.is_empty() {
        return Err(ParseError::FindChecksum {
            name: artifact.to_owned(),
        });
    }
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
    let after_separator = rest.trim_start_matches([' ', '\t']);
    let separator_len = rest.len() - after_separator.len();
    let leading_name_space = artifact.len() - artifact.trim_start_matches([' ', '\t']).len();
    let name_start = if leading_name_space == artifact.len() && after_separator.is_empty() {
        rest.find(artifact)
    } else {
        separator_len.checked_sub(leading_name_space)
    };
    let without_marker = name_start.map(|start| &rest[start..]);
    let with_marker = after_separator.strip_prefix('*');
    [without_marker, with_marker]
        .into_iter()
        .flatten()
        .any(|name| {
            name.strip_prefix(artifact)
                .is_some_and(|suffix| suffix.bytes().all(|byte| matches!(byte, b' ' | b'\t')))
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
