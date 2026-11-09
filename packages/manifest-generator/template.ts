// The authored template of the cores clash-nyanpasu ships (design §9.4).
//
// Each platform has one build that runs on clash-nyanpasu's minimum systems:
// macOS 12.6, Windows 10, Linux with glibc 2.35, and x86_64 CPUs with AES-NI
// (hence x86-64-v2). Every resource has a stable and an alpha variant tagged
// only by channel.
import type {
  RemoteChecksumResourceTemplate,
  RemoteVersionTemplate,
  ResourceManifestTemplate,
  ResourceTemplate,
  SupportedPlatform,
} from "@manifest/metadata";

/** Captured stable releases per resource. */
export const HISTORY_LIMIT = 8;

type Artifacts = Record<SupportedPlatform, string>;

const GITHUB_ASSETS_CHECKSUM: RemoteChecksumResourceTemplate = {
  resource: { registry: "github-release-metadata" },
  parser: { type: "github-release-assets" },
};

const CHANNEL_TAGS: ResourceTemplate["tags"] = {
  "channel:stable": { name: "Stable", group: "channel" },
  "channel:alpha": { name: "Alpha", group: "channel" },
};

function escapeRegex(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function resource(options: {
  name: string;
  owner: string;
  repo: string;
  alphaTag: string;
  artifacts: Artifacts;
  alphaVersion: (artifact: string) => RemoteVersionTemplate;
}): ResourceTemplate {
  const platforms = Object.keys(options.artifacts) as SupportedPlatform[];
  return {
    name: options.name,
    registry: "github-release",
    source: { owner: options.owner, repo: options.repo },
    tags: CHANNEL_TAGS,
    default_variants: Object.fromEntries(
      platforms.map((platform) => [platform, "stable"]),
    ),
    releases: {
      stable: { type: "history", limit: HISTORY_LIMIT },
      alpha: { type: "tag", tag: options.alphaTag },
    },
    variants: [
      {
        id: "stable",
        name: "Stable",
        tags: ["channel:stable"],
        source: { release: "{version}" },
        artifacts: Object.fromEntries(
          platforms.map((platform) => [
            platform,
            {
              artifact: options.artifacts[platform],
              version: { type: "static", releases: "stable" },
            },
          ]),
        ),
      },
      {
        id: "alpha",
        name: "Alpha",
        tags: ["channel:alpha"],
        source: { release: options.alphaTag },
        artifacts: Object.fromEntries(
          platforms.map((platform) => [
            platform,
            {
              artifact: options.artifacts[platform],
              version: {
                type: "remote",
                releases: "alpha",
                version: options.alphaVersion(options.artifacts[platform]),
                checksum: GITHUB_ASSETS_CHECKSUM,
              },
            },
          ]),
        ),
      },
    ],
  };
}

/** Mihomo publishes the rolling alpha version in `version.txt`. */
const mihomo = resource({
  name: "Mihomo",
  owner: "MetaCubeX",
  repo: "mihomo",
  alphaTag: "Prerelease-Alpha",
  artifacts: {
    "windows-x86_64": "mihomo-windows-amd64-v2-{version}.zip",
    "windows-arm64": "mihomo-windows-arm64-{version}.zip",
    "linux-aarch64": "mihomo-linux-arm64-{version}.gz",
    "linux-amd64": "mihomo-linux-amd64-v2-{version}.gz",
    "darwin-arm64": "mihomo-darwin-arm64-{version}.gz",
    "darwin-x64": "mihomo-darwin-amd64-v2-{version}.gz",
  },
  alphaVersion: () => ({
    resource: { registry: "github-release", artifact: "version.txt" },
    parser: { type: "text", trim: true },
  }),
});

/** Meow has no version file; the alpha version is this platform's asset name. */
const meow = resource({
  name: "Meow",
  owner: "meow-rs",
  repo: "meow-rs",
  alphaTag: "Prerelease-Alpha",
  artifacts: {
    "windows-x86_64": "meow-{version}-x86_64-pc-windows-msvc.zip",
    "windows-arm64": "meow-{version}-aarch64-pc-windows-msvc.zip",
    "linux-aarch64": "meow-{version}-aarch64-unknown-linux-musl.tar.gz",
    "linux-amd64": "meow-{version}-x86_64-unknown-linux-musl.tar.gz",
    "darwin-arm64": "meow-{version}-aarch64-apple-darwin.tar.gz",
    "darwin-x64": "meow-{version}-x86_64-apple-darwin.tar.gz",
  },
  alphaVersion: (artifact) => {
    const [prefix, suffix] = artifact.split("{version}") as [string, string];
    return {
      resource: { registry: "github-release-metadata" },
      parser: {
        type: "regex",
        pattern: `${escapeRegex(prefix)}(alpha-[0-9a-f]{7,40})${escapeRegex(suffix)}`,
      },
    };
  },
});

/**
 * Clash Rust's alpha version combines the latest stable tag with the commit
 * behind the rolling `latest` tag (design §9.2). The dynamic linux-gnu builds
 * need glibc 2.39, so Linux uses static-crt on x86_64 and musl on aarch64.
 */
const clashRs = resource({
  name: "Clash Rust",
  owner: "ibigbug",
  repo: "clash-rs",
  alphaTag: "latest",
  artifacts: {
    "windows-x86_64": "clash-rs-x86_64-pc-windows-msvc.exe",
    "windows-arm64": "clash-rs-aarch64-pc-windows-msvc.exe",
    "linux-aarch64": "clash-rs-aarch64-unknown-linux-musl",
    "linux-amd64": "clash-rs-x86_64-unknown-linux-gnu-static-crt",
    "darwin-arm64": "clash-rs-aarch64-apple-darwin",
    "darwin-x64": "clash-rs-x86_64-apple-darwin",
  },
  alphaVersion: () => ({
    parts: {
      stable: {
        resource: { registry: "github-api", artifact: "releases/latest" },
        parser: { type: "regex", pattern: '"tag_name":\\s*"v?([0-9][^"]*)"' },
      },
      commit: {
        resource: { registry: "github-api", artifact: "commits/{release}" },
        parser: {
          type: "regex",
          pattern: '"sha":\\s*"([0-9a-f]{7})[0-9a-f]{33}"',
        },
      },
    },
    format: "{stable}-alpha+sha.{commit}",
  }),
});

export const TEMPLATE: ResourceManifestTemplate = {
  manifest_version: "2.0",
  registries: {
    "github-api": {
      name: "GitHub REST API",
      url_template: "https://api.github.com/repos/{owner}/{repo}/{artifact}",
    },
    "github-release": {
      name: "GitHub Releases",
      url_template:
        "https://github.com/{owner}/{repo}/releases/download/{release}/{artifact}",
    },
    "github-release-metadata": {
      name: "GitHub Release Assets Metadata",
      url_template:
        "https://github.com/{owner}/{repo}/releases/expanded_assets/{release}",
    },
  },
  mirrors: {
    direct: {
      name: "Direct",
      url_template: "{url}",
      registries: ["github-api", "github-release", "github-release-metadata"],
      priority: 0,
    },
    // Rewriting `{path_and_query}` drops the api.github.com host, so this
    // mirror serves only github.com.
    nyanpasu: {
      name: "Nyanpasu GitHub mirror",
      url_template: "https://nyanpasu-script.majokeiko.com{path_and_query}",
      registries: ["github-release", "github-release-metadata"],
      priority: 10,
    },
  },
  resources: { clash_rs: clashRs, meow, mihomo },
};
