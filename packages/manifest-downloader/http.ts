import { createHash } from "node:crypto";
import { createWriteStream } from "node:fs";
import { Readable, Transform } from "node:stream";
import { pipeline } from "node:stream/promises";
import type {
  ChecksumAlgorithm,
  RemoteChecksumParser,
  RemoteVersionParser,
} from "@manifest/metadata";
import {
  ManifestRuntimeError,
  type RemoteResponseInput,
} from "@manifest/runtime";

export const BODY_LIMIT = 2 * 1024 * 1024;
export const HEADER_VALUE_LIMIT = 8 * 1024;
export const DEFAULT_TIMEOUT_MS = 30_000;
const MAX_REDIRECTS = 5;
type HttpLocator =
  "http.manifest" | "http.version" | "http.checksum" | "http.artifact";

function redactUrl(url: string): string {
  try {
    const target = new URL(url);
    return `${target.protocol}//${target.host}${target.pathname}${target.search ? "?<redacted>" : ""}`;
  } catch {
    return "<invalid URL>";
  }
}

export function hostError(
  code: string,
  message: string,
  path: string,
  fields: Record<string, unknown> = {},
): ManifestRuntimeError {
  return new ManifestRuntimeError({ code, message, path, fields });
}

function sizeError(
  path: string,
  limit: number,
  actual: number,
): ManifestRuntimeError {
  return hostError(
    "parser.body_too_large",
    `response exceeds ${limit} bytes (received ${actual})`,
    path,
    { limit, actual },
  );
}

async function request<T>(
  url: string,
  timeoutMs: number,
  path: HttpLocator,
  consume: (response: Response) => Promise<T>,
): Promise<T> {
  let current = url;
  let signal: AbortSignal | undefined;
  try {
    for (let redirects = 0; ; redirects++) {
      const target = new URL(current);
      if (
        !["http:", "https:"].includes(target.protocol) ||
        target.username ||
        target.password ||
        target.hash
      ) {
        throw hostError(
          "http.invalid_url",
          "HTTP URL must use http(s) without userinfo or fragment",
          path,
        );
      }
      signal = AbortSignal.timeout(timeoutMs);
      const response = await globalThis.fetch(target, {
        redirect: "manual",
        signal,
      });
      try {
        const status = response.status;
        if ([301, 302, 303, 307, 308].includes(status)) {
          const location = response.headers.get("location");
          if (!location)
            throw hostError(
              "http.redirect_location_missing",
              "redirect has no Location header",
              path,
            );
          if (redirects === MAX_REDIRECTS)
            throw hostError(
              "http.redirect_limit",
              `more than ${MAX_REDIRECTS} redirects`,
              path,
            );
          if (Buffer.byteLength(location) > HEADER_VALUE_LIMIT)
            throw sizeError(
              "response.headers.location",
              HEADER_VALUE_LIMIT,
              Buffer.byteLength(location),
            );
          const redirected = new URL(location, target);
          if (target.protocol === "https:" && redirected.protocol === "http:") {
            throw hostError(
              "http.insecure_redirect",
              "HTTPS redirect must not downgrade to HTTP",
              path,
            );
          }
          current = redirected.href;
          continue;
        }
        if (status < 200 || status >= 300)
          throw hostError(
            "http.status",
            `HTTP ${status} from ${redactUrl(current)}`,
            path,
          );
        return await consume(response);
      } finally {
        if (response.body && !response.body.locked) {
          await response.body.cancel().catch(() => {});
        }
      }
    }
  } catch (error) {
    if (error instanceof ManifestRuntimeError) throw error;
    const timedOut = signal?.aborted ?? false;
    throw hostError(
      timedOut ? "http.timeout" : "http.request_failed",
      `HTTP request ${timedOut ? "timed out" : "failed"}: ${redactUrl(current)}`,
      path,
    );
  }
}

export async function readResponse(
  url: string,
  parser?: RemoteChecksumParser | RemoteVersionParser,
  timeoutMs = DEFAULT_TIMEOUT_MS,
  path: HttpLocator = "http.manifest",
): Promise<RemoteResponseInput> {
  return request(url, timeoutMs, path, async (response) => {
    const headers: [string, string][] = [];
    if (parser?.type === "header") {
      const name = parser.name.toLowerCase();
      const value = response.headers.get(name);
      if (value !== null) {
        const bytes = Buffer.byteLength(value);
        if (bytes > HEADER_VALUE_LIMIT)
          throw sizeError(
            `response.headers.${name}`,
            HEADER_VALUE_LIMIT,
            bytes,
          );
        headers.push([name, value]);
      }
    }
    const chunks: Uint8Array[] = [];
    let bytes = 0;
    for await (const chunk of response.body ?? []) {
      bytes += chunk.byteLength;
      if (bytes > BODY_LIMIT)
        throw sizeError("response.body", BODY_LIMIT, bytes);
      chunks.push(chunk);
    }
    return { body: Buffer.concat(chunks, bytes).toString("utf8"), headers };
  });
}

export async function downloadArtifact(
  url: string,
  path: string,
  algorithm: ChecksumAlgorithm,
  maxBytes: number,
  timeoutMs = DEFAULT_TIMEOUT_MS,
): Promise<string> {
  return request(url, timeoutMs, "http.artifact", async (response) => {
    if (!response.body) {
      throw hostError(
        "http.empty_body",
        "artifact response has no body",
        "http.artifact",
      );
    }
    const hash = createHash(algorithm);
    let bytes = 0;
    await pipeline(
      Readable.fromWeb(response.body),
      new Transform({
        transform(chunk: Buffer, _encoding, callback) {
          bytes += chunk.byteLength;
          if (bytes > maxBytes) {
            callback(
              hostError(
                "download.artifact_too_large",
                `artifact exceeds ${maxBytes} bytes`,
                "http.artifact",
                { limit: maxBytes, actual: bytes },
              ),
            );
            return;
          }
          hash.update(chunk);
          callback(null, chunk);
        },
      }),
      createWriteStream(path, { flags: "wx" }),
    );
    return hash.digest("hex");
  });
}
