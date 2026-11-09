// The authored template of the cores clash-nyanpasu ships (design §9.4).
//
// clash-nyanpasu's minimum systems are the floor: macOS 12.6, Windows 10,
// Linux with glibc 2.35, and x86_64 CPUs with AES-NI (hence x86-64-v2). The
// default `stable` and `alpha` variants hold one build per platform that runs
// there. Upstream builds that need more or link differently are optional
// variants, tagged with `cpu`, `libc` or `crt` on top of the channel tag, for a
// host to offer after detecting the CPU or system.
import type {
  RemoteChecksumResourceTemplate,
  RemoteVersionTemplate,
  ResourceManifestTemplate,
  ResourceTemplate,
  ResourceVariantTemplate,
  SupportedPlatform,
} from "@manifest/metadata";

/** Captured stable releases per resource. */
export const HISTORY_LIMIT = 8;

type Artifacts = Partial<Record<SupportedPlatform, string>>;
type TagCatalog = ResourceTemplate["tags"];

/** Builds that differ from the default ones, offered under one extra tag. */
interface OptionalBuild {
  /** Appended to the channel in the variant ID and name. */
  suffix: string;
  name: string;
  tag: string;
  /** Only the platforms whose build differs from the default. */
  artifacts: Artifacts;
}

const GITHUB_ASSETS_CHECKSUM: RemoteChecksumResourceTemplate = {
  resource: { registry: "github-release-metadata" },
  parser: { type: "github-release-assets" },
};

const CHANNELS = [
  { id: "stable", name: "Stable", tag: "channel:stable" },
  { id: "alpha", name: "Alpha", tag: "channel:alpha" },
] as const;

const CHANNEL_TAGS: TagCatalog = {
  "channel:stable": { name: "Stable", group: "channel" },
  "channel:alpha": { name: "Alpha", group: "channel" },
};

const X86_64_V3: TagCatalog = {
  "cpu:x86-64-v3": {
    name: "x86-64-v3",
    group: "cpu",
    description: "Requires x86-64-v3: AVX2, BMI1, BMI2, FMA, LZCNT and MOVBE.",
  },
};

const GLIBC = (version: string): TagCatalog => ({
  "libc:gnu": {
    name: "glibc",
    group: "libc",
    description: `Dynamically linked against glibc ${version} or later.`,
  },
});

const MUSL: TagCatalog = {
  "libc:musl": {
    name: "musl",
    group: "libc",
    description: "Statically linked against musl.",
  },
};

const STATIC_CRT: TagCatalog = {
  "crt:static": {
    name: "Static CRT",
    group: "crt",
    description:
      "Upstream's static-crt build. On Windows the C runtime is linked statically, so no Visual C++ runtime is needed; on macOS the system library libSystem is still linked dynamically.",
  },
};

function escapeRegex(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function resource(options: {
  name: string;
  owner: string;
  repo: string;
  alphaTag: string;
  /** The default builds, one per platform. */
  artifacts: Artifacts;
  optional?: OptionalBuild[];
  tags?: TagCatalog;
  alphaVersion: (artifact: string) => RemoteVersionTemplate;
}): ResourceTemplate {
  const flavors = [
    { suffix: null, name: null, tag: null, artifacts: options.artifacts },
    ...(options.optional ?? []),
  ];
  const variants = CHANNELS.flatMap((channel) =>
    flavors.map((flavor): ResourceVariantTemplate => ({
      id: flavor.suffix ? `${channel.id}-${flavor.suffix}` : channel.id,
      name: flavor.name ? `${channel.name} (${flavor.name})` : channel.name,
      tags: flavor.tag ? [channel.tag, flavor.tag] : [channel.tag],
      source: {
        release: channel.id === "stable" ? "{version}" : options.alphaTag,
      },
      artifacts: Object.fromEntries(
        Object.entries(flavor.artifacts).map(([platform, artifact]) => [
          platform,
          {
            artifact,
            version:
              channel.id === "stable"
                ? { type: "static", releases: "stable" }
                : {
                    type: "remote",
                    releases: "alpha",
                    latest: options.alphaVersion(artifact),
                    checksum: GITHUB_ASSETS_CHECKSUM,
                  },
          },
        ]),
      ),
    })),
  );

  return {
    name: options.name,
    registry: "github-release",
    source: { owner: options.owner, repo: options.repo },
    tags: { ...CHANNEL_TAGS, ...options.tags },
    // Every platform has several variants, so each default is explicit.
    default_variants: Object.fromEntries(
      Object.keys(options.artifacts).map((platform) => [platform, "stable"]),
    ),
    releases: {
      stable: { type: "history", limit: HISTORY_LIMIT },
      alpha: { type: "tag", tag: options.alphaTag },
    },
    variants,
  };
}

/**
 * Mihomo publishes the rolling alpha version in `version.txt`. Its plain
 * `amd64` builds are `amd64-v3` under an older name.
 */
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
  optional: [
    {
      suffix: "x86-64-v3",
      name: "x86-64-v3",
      tag: "cpu:x86-64-v3",
      artifacts: {
        "windows-x86_64": "mihomo-windows-amd64-v3-{version}.zip",
        "linux-amd64": "mihomo-linux-amd64-v3-{version}.gz",
        "darwin-x64": "mihomo-darwin-amd64-v3-{version}.gz",
      },
    },
  ],
  tags: X86_64_V3,
  alphaVersion: () => ({
    resource: { registry: "github-release", artifact: "version.txt" },
    parser: { type: "text", trim: true },
  }),
});

/**
 * Meow has no version file; the alpha version is this platform's asset name.
 * Linux defaults to musl as clash-nyanpasu does; the glibc builds need 2.28.
 */
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
  optional: [
    {
      suffix: "gnu",
      name: "glibc",
      tag: "libc:gnu",
      artifacts: {
        "linux-aarch64": "meow-{version}-aarch64-unknown-linux-gnu.tar.gz",
        "linux-amd64": "meow-{version}-x86_64-unknown-linux-gnu.tar.gz",
      },
    },
  ],
  tags: GLIBC("2.28"),
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
 * need glibc 2.39, so by default Linux uses static-crt on x86_64 and musl on
 * aarch64; the glibc builds are optional.
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
  optional: [
    {
      suffix: "gnu",
      name: "glibc",
      tag: "libc:gnu",
      artifacts: {
        "linux-aarch64": "clash-rs-aarch64-unknown-linux-gnu",
        "linux-amd64": "clash-rs-x86_64-unknown-linux-gnu",
      },
    },
    {
      suffix: "musl",
      name: "musl",
      tag: "libc:musl",
      artifacts: { "linux-amd64": "clash-rs-x86_64-unknown-linux-musl" },
    },
    {
      suffix: "static-crt",
      name: "static CRT",
      tag: "crt:static",
      artifacts: {
        "windows-x86_64": "clash-rs-x86_64-pc-windows-msvc-static-crt.exe",
        "windows-arm64": "clash-rs-aarch64-pc-windows-msvc-static-crt.exe",
        "darwin-arm64": "clash-rs-aarch64-apple-darwin-static-crt",
        "darwin-x64": "clash-rs-x86_64-apple-darwin-static-crt",
      },
    },
  ],
  tags: { ...GLIBC("2.39"), ...MUSL, ...STATIC_CRT },
  alphaVersion: () => ({
    parts: {
      stable: {
        resource: { registry: "github-api", artifact: "releases/latest" },
        parser: {
          type: "json",
          pointer: "/tag_name",
          pattern: "^v?([0-9].*)$",
        },
      },
      commit: {
        resource: { registry: "github-api", artifact: "commits/{release}" },
        parser: {
          type: "json",
          pointer: "/sha",
          pattern: "^([0-9a-f]{7})[0-9a-f]{33}$",
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
