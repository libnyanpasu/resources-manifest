// All generator HTTP requests. Bodies are stream-counted against a limit, each
// request has one timeout covering the body, and the GitHub token is sent only
// to the GitHub REST API host.

/** Remote version responses; matches the core parser body limit (design §5.5). */
export const VERSION_BODY_LIMIT = 2 * 1024 * 1024;
/** GitHub REST API pages: a release page lists every asset inline. */
export const API_BODY_LIMIT = 16 * 1024 * 1024;
export const DEFAULT_TIMEOUT_MS = 30_000;
const GITHUB_API_ORIGIN = "https://api.github.com";
const USER_AGENT = "manifest-generator";

export interface HttpOptions {
  /** Sent as a bearer token to `https://api.github.com` only. */
  token?: string;
  timeoutMs?: number;
}

function redactUrl(url: string): string {
  try {
    const target = new URL(url);
    return `${target.protocol}//${target.host}${target.pathname}${target.search ? "?<redacted>" : ""}`;
  } catch {
    return "<invalid URL>";
  }
}

async function get(
  url: string,
  accept: string,
  limit: number,
  options: HttpOptions,
): Promise<string> {
  const headers: Record<string, string> = {
    Accept: accept,
    "User-Agent": USER_AGENT,
  };
  if (options.token && new URL(url).origin === GITHUB_API_ORIGIN) {
    headers.Authorization = `Bearer ${options.token}`;
    headers["X-GitHub-Api-Version"] = "2022-11-28";
  }
  const signal = AbortSignal.timeout(options.timeoutMs ?? DEFAULT_TIMEOUT_MS);

  let response: Response;
  try {
    response = await globalThis.fetch(url, { headers, signal });
  } catch {
    throw new Error(
      `HTTP request ${signal.aborted ? "timed out" : "failed"}: ${redactUrl(url)}`,
    );
  }
  if (!response.ok) {
    await response.body?.cancel().catch(() => {});
    throw new Error(`HTTP ${response.status} from ${redactUrl(url)}`);
  }

  const chunks: Uint8Array[] = [];
  let bytes = 0;
  try {
    for await (const chunk of response.body ?? []) {
      bytes += chunk.byteLength;
      if (bytes > limit) {
        throw new Error(
          `response from ${redactUrl(url)} exceeds ${limit} bytes`,
        );
      }
      chunks.push(chunk);
    }
  } finally {
    if (response.body && !response.body.locked) {
      await response.body.cancel().catch(() => {});
    }
  }
  return Buffer.concat(chunks, bytes).toString("utf8");
}

/** Reads a response body as text, as a host does for a version request. */
export function getText(url: string, options: HttpOptions): Promise<string> {
  return get(url, "*/*", VERSION_BODY_LIMIT, options);
}

/**
 * Reads a GitHub REST API resource. `id` fields keep their source digits as
 * strings: a JS number cannot hold every GitHub ID.
 */
export async function getJson<T>(
  url: string,
  options: HttpOptions,
): Promise<T> {
  const body = await get(
    url,
    "application/vnd.github+json",
    API_BODY_LIMIT,
    options,
  );
  const parse = JSON.parse as (
    text: string,
    reviver: (
      key: string,
      value: unknown,
      context: { source?: string },
    ) => unknown,
  ) => unknown;
  return parse(body, (key, value, context) =>
    key === "id" && typeof value === "number" ? context.source : value,
  ) as T;
}
