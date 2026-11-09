import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import {
  mkdtemp,
  mkdir,
  readFile,
  readdir,
  rm,
  writeFile,
} from "node:fs/promises";
import { createServer } from "node:http";
import { join } from "node:path";
import { test, type TestContext } from "node:test";
import { createServer as createTlsServer, type TLSSocket } from "node:tls";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import type {
  RemoteChecksumParser,
  ResourceArtifactVersionTemplate,
  ResourceManifest,
} from "@manifest/metadata";
import { ManifestRuntimeError } from "@manifest/runtime";
import { BODY_LIMIT, HEADER_VALUE_LIMIT, readResponse } from "./http.ts";
import { run, type RunDependencies } from "./main.ts";

const payload = Buffer.from("synthetic artifact\n");
const digest = createHash("sha256").update(payload).digest("hex");
const checksum = `sha256:${digest}` as const;

const tlsKey = `-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgu/6FgK9f9RxnzOh1
pub7WhtfF4yZmyUlHfIifaQgOe+hRANCAARh5GhKamW2gZSVeQPEFwWMul8OybQO
Eh6xr9RrUuMBlQQpxXgkaaaTmsue8SbakwuGXAnW4BKXNoyGY4ZbuTLv
-----END PRIVATE KEY-----
`;
const tlsCert = `-----BEGIN CERTIFICATE-----
MIIBezCCASGgAwIBAgIUKFs8JcIfTMeibBpCND8iKFCqJGswCgYIKoZIzj0EAwIw
GjEYMBYGA1UEAwwPZG93bmxvYWRlciB0ZXN0MCAXDTI2MTEwNzA5MTEwN1oYDzIx
MjYxMDE0MDkxMTA3WjAaMRgwFgYDVQQDDA9kb3dubG9hZGVyIHRlc3QwWTATBgcq
hkjOPQIBBggqhkjOPQMBBwNCAARh5GhKamW2gZSVeQPEFwWMul8OybQOEh6xr9Rr
UuMBlQQpxXgkaaaTmsue8SbakwuGXAnW4BKXNoyGY4ZbuTLvo0MwQTAPBgNVHREE
CDAGhwR/AAABMA8GA1UdEwEB/wQFMAMBAf8wHQYDVR0OBBYEFAmE3/HkwYpqWcJl
eQLo5p9h1sA3MAoGCCqGSM49BAMCA0gAMEUCIQC/zVMAGzni9s8On2v4BNR+Nvno
BFa45eWFKH55rpyE+gIgDMwZRyIQkDTQeMGH1hkEVVdUR3eJVP62IfIrfIc+9vE=
-----END CERTIFICATE-----
`;

interface Route {
  body?: string | Buffer;
  headers?: Record<string, string | string[]>;
  status?: number;
  stall?: boolean;
  stallHeaders?: boolean;
  disconnect?: boolean;
}

async function fixture(t: TestContext, remote = false) {
  const directory = await mkdtemp(
    fileURLToPath(new URL(".download-test-", import.meta.url)),
  );
  t.after(() => rm(directory, { recursive: true, force: true }));
  const out = join(directory, "out");
  await mkdir(out);
  const manifestPath = join(directory, "manifest.json");
  const logs: string[] = [];
  const requests: string[] = [];
  const socketClosures = new Map<string, Promise<void>>();
  const socketClosuresBySocket = new WeakMap<object, Promise<void>>();
  const events: { type: "request" | "log"; value: string; at: bigint }[] = [];
  const routes: Record<string, Route> = {
    "/version.txt": { body: " v1\n" },
    "/checksums.txt": { body: checksum },
    "/pkg-v1.zip": { body: payload },
  };
  const server = createServer((request, response) => {
    const path = request.url!;
    requests.push(path);
    let closed = socketClosuresBySocket.get(request.socket);
    if (!closed) {
      closed = new Promise<void>((resolve) =>
        request.socket.once("close", () => resolve()),
      );
      socketClosuresBySocket.set(request.socket, closed);
    }
    socketClosures.set(path, closed);
    events.push({ type: "request", value: path, at: process.hrtime.bigint() });
    const redirect = /^\/redirect\/(\d+)(\/.*)$/.exec(path);
    if (redirect) {
      const remaining = Number(redirect[1]);
      response.writeHead(302, {
        Location:
          remaining > 1
            ? `/redirect/${remaining - 1}${redirect[2]}`
            : redirect[2],
      });
      response.end();
      return;
    }
    const route = routes[path];
    if (route?.stallHeaders) return;
    response.writeHead(route?.status ?? (route ? 200 : 404), route?.headers);
    if (route?.stall) {
      response.flushHeaders();
      return;
    }
    if (route?.disconnect) {
      response.write(payload.subarray(0, 2));
      setImmediate(() => response.destroy());
      return;
    }
    response.end(route?.body ?? "missing");
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(
    () =>
      new Promise<void>((resolve, reject) => {
        server.close((error) => (error ? reject(error) : resolve()));
        server.closeAllConnections();
      }),
  );
  const address = server.address();
  assert.ok(address && typeof address === "object");
  const base = `http://127.0.0.1:${address.port}`;
  const version: ResourceArtifactVersionTemplate = remote
    ? {
        type: "remote",
        fallback: { version: "v1", checksum },
        latest: {
          resource: { registry: "main", artifact: "version.txt" },
          parser: { type: "text" },
        },
        checksum: {
          resource: { registry: "main", artifact: "checksums.txt" },
          parser: { type: "checksum" },
        },
      }
    : {
        type: "static",
        latest: "v1",
        releases: [{ version: "v1", checksum }],
      };
  const manifest: ResourceManifest = {
    manifest_version: "2.0",
    registries: { main: { name: "Main", url_template: `${base}/{artifact}` } },
    mirrors: { proxy: { name: "Proxy", url_template: `${base}/mirror{path}` } },
    resources: {
      sample: {
        name: "Sample",
        registry: "main",
        tags: [],
        default_variants: { "linux-amd64": "stable" },
        variants: [
          {
            id: "stable",
            name: "Stable",
            tag_key: "",
            tags: [],
            artifacts: {
              "linux-amd64": { artifact: "pkg-{version}.zip", version },
            },
          },
        ],
      },
    },
  };
  const argv = [
    "--manifest",
    manifestPath,
    "--resource",
    "sample",
    "--platform",
    "linux-amd64",
    "--out",
    out,
  ];
  return {
    base,
    out,
    manifest,
    version,
    routes,
    requests,
    socketClosures,
    events,
    logs,
    argv,
    async save() {
      await writeFile(manifestPath, JSON.stringify(manifest));
    },
    async run(extra: string[] = [], deps: RunDependencies = {}) {
      await writeFile(manifestPath, JSON.stringify(manifest));
      return run([...argv, ...extra], {
        log: (line) => {
          logs.push(line);
          events.push({
            type: "log",
            value: line,
            at: process.hrtime.bigint(),
          });
        },
        ...deps,
      });
    },
  };
}

function rejectsWith(code: string, path?: string) {
  return (error: unknown) => {
    assert.ok(error instanceof ManifestRuntimeError);
    assert.equal(error.code, code);
    if (path) assert.equal(error.path, path);
    return true;
  };
}

test("environment proxy routes requests and NO_PROXY bypasses it", async (t) => {
  const [major, minor] = process.versions.node.split(".").map(Number);
  if (major < 24 && (major !== 22 || minor < 21)) {
    t.skip("fetch environment proxy support requires Node.js >= 24.0 or 22.21");
    return;
  }
  const cases = [
    ["proxy variables alone stay direct", "", "", [], false, false],
    ["NODE_USE_ENV_PROXY=0 stays direct", "0", "", [], false, false],
    ["HTTP_PROXY is used", "1", "", [], false, true],
    ["NO_PROXY goes direct", "1", "", [], true, true],
    ["execArgv enables proxy", "0", "", ["--use-env-proxy"], false, true],
    ["NODE_OPTIONS enables proxy", "", "--use-env-proxy", [], false, true],
    ["quoted NODE_OPTIONS flag", "", '"--use-env-proxy"', [], false, true],
    ["underscored flag", "", "--use_env_proxy", [], false, true],
    [
      "NODE_OPTIONS disables env enablement",
      "1",
      "--no-use-env-proxy",
      [],
      false,
      false,
    ],
    [
      "execArgv overrides NODE_OPTIONS",
      "1",
      "--use-env-proxy",
      ["--no-use-env-proxy"],
      false,
      false,
    ],
    [
      "execArgv overrides NODE_OPTIONS disablement",
      "",
      "--no-use-env-proxy",
      ["--use-env-proxy"],
      false,
      true,
    ],
    [
      "last flag wins",
      "",
      "--use-env-proxy --no-use-env-proxy",
      [],
      false,
      false,
    ],
  ] as const;
  for (const [name, envValue, nodeOptions, args, bypass, enabled] of cases) {
    await t.test(name, async (t) => {
      if (major === 24 && minor < 5 && (nodeOptions || args.length)) {
        t.skip("--use-env-proxy requires Node.js >= 24.5");
        return;
      }
      const origin = await fixture(t);
      const proxy = await fixture(t);
      const target = `${origin.base}/version.txt`;
      proxy.routes[target] = { body: "proxied" };
      const env = Object.fromEntries(
        Object.entries(process.env).filter(
          ([name]) =>
            !/^(?:(?:https?|all|no)_proxy|node_use_env_proxy)$/i.test(name),
        ),
      );
      const { stdout } = await promisify(execFile)(
        process.execPath,
        [
          ...args,
          "--input-type=module",
          "-e",
          `import { readResponse } from ${JSON.stringify(new URL("http.ts", import.meta.url).href)};
           console.log((await readResponse(process.argv[1])).body);`,
          target,
        ],
        {
          env: {
            ...env,
            NODE_USE_ENV_PROXY: envValue,
            NODE_OPTIONS: nodeOptions,
            HTTP_PROXY: proxy.base,
            NO_PROXY: bypass ? "127.0.0.1" : "",
          },
          timeout: 10_000,
        },
      );
      const proxied = enabled && !bypass;
      assert.equal(stdout.trim(), proxied ? "proxied" : "v1");
      assert.deepEqual(proxy.requests, proxied ? [target] : []);
      assert.deepEqual(origin.requests, proxied ? [] : ["/version.txt"]);
    });
  }
});

test("static artifact verifies before rename and reports authoritative policy", async (t) => {
  const f = await fixture(t);
  const result = await f.run(["--variant", "stable"], {
    log(line) {
      assert.deepEqual(f.requests, []);
      f.logs.push(line);
    },
  });
  assert.equal(result, join(f.out, "pkg-v1.zip"));
  assert.deepEqual(await readFile(result), payload);
  assert.deepEqual(await readdir(f.out), ["pkg-v1.zip"]);
  assert.match(
    f.logs.join("\n"),
    /resource=sample variant=stable version=v1 kind=authoritative policy=accept reason=/,
  );
});

test("default policy rejects remote before any metadata or artifact request", async (t) => {
  const f = await fixture(t, true);
  await assert.rejects(f.run(), (error) => {
    rejectsWith("verify.kind_rejected", "arg.kind")(error);
    assert.match((error as Error).message, /--allow-parsed/);
    return true;
  });
  assert.match(
    f.logs.join("\n"),
    /kind=parsed policy=reject reason=verify.kind_rejected/,
  );
  assert.deepEqual(f.requests, []);
  assert.deepEqual(await readdir(f.out), []);
});

test("allow-parsed resolves version and checksum then downloads with WARN", async (t) => {
  const f = await fixture(t, true);
  await f.run(["--allow-parsed"]);
  assert.deepEqual(f.requests, [
    "/version.txt",
    "/checksums.txt",
    "/pkg-v1.zip",
  ]);
  assert.match(f.logs.join("\n"), /version=v1 kind=parsed policy=accept/);
  assert.ok(
    f.logs.some(
      (line) => line.startsWith("WARN") && line.includes("--allow-parsed"),
    ),
  );
  assert.deepEqual(await readFile(join(f.out, "pkg-v1.zip")), payload);
});

test("composite versions request every part in plan order", async (t) => {
  const f = await fixture(t, true);
  assert.equal(f.version.type, "remote");
  f.version.latest = {
    parts: {
      major: {
        resource: { registry: "main", artifact: "major.txt" },
        parser: { type: "text" },
      },
      build: {
        resource: { registry: "main", artifact: "build.txt" },
        parser: { type: "text" },
      },
    },
    format: "{major}{build}",
  };
  f.routes["/major.txt"] = { body: "v" };
  f.routes["/build.txt"] = { body: "1\n" };
  await f.run(["--allow-parsed"]);
  assert.deepEqual(f.requests, [
    "/build.txt",
    "/major.txt",
    "/checksums.txt",
    "/pkg-v1.zip",
  ]);
  assert.match(f.logs.join("\n"), /version=v1 kind=parsed policy=accept/);
  assert.deepEqual(await readFile(join(f.out, "pkg-v1.zip")), payload);
});

test("digest mismatch removes temporary files and preserves existing output", async (t) => {
  const f = await fixture(t);
  f.routes["/pkg-v1.zip"].body = "corrupted";
  await writeFile(join(f.out, "existing.txt"), "keep");
  await assert.rejects(
    f.run(),
    rejectsWith("verify.digest_mismatch", "arg.actual"),
  );
  assert.deepEqual(await readdir(f.out), ["existing.txt"]);
  assert.equal(await readFile(join(f.out, "existing.txt"), "utf8"), "keep");
});

test("rename failure preserves an existing destination directory and removes temporary files", async (t) => {
  const f = await fixture(t);
  const destination = join(f.out, "pkg-v1.zip");
  await mkdir(destination);
  await writeFile(join(destination, "keep.txt"), "directory contents");
  await writeFile(join(f.out, "existing.txt"), "keep");
  await assert.rejects(f.run(), (error: unknown) => {
    assert.ok(error instanceof Error && "code" in error);
    assert.ok(
      ["EISDIR", "EPERM", "EEXIST", "ENOTEMPTY"].includes(String(error.code)),
    );
    return true;
  });
  assert.deepEqual(f.requests, ["/pkg-v1.zip"]);
  assert.deepEqual((await readdir(f.out)).sort(), [
    "existing.txt",
    "pkg-v1.zip",
  ]);
  assert.deepEqual(await readdir(destination), ["keep.txt"]);
  assert.equal(
    await readFile(join(destination, "keep.txt"), "utf8"),
    "directory contents",
  );
  assert.equal(await readFile(join(f.out, "existing.txt"), "utf8"), "keep");
});

test("throwing policy log callbacks preserve existing files and leave no temporary files", async (t) => {
  for (const throwAt of [1, 2, 3, 4]) {
    const f = await fixture(t, true);
    await writeFile(join(f.out, "pkg-v1.zip"), "existing artifact");
    await writeFile(join(f.out, "existing.txt"), "keep");
    const failure = new Error("log callback failed");
    let calls = 0;
    await assert.rejects(
      f.run(["--allow-parsed"], {
        log() {
          if (++calls === throwAt) throw failure;
        },
      }),
      (error) => error === failure,
    );
    assert.equal(calls, throwAt);
    assert.deepEqual(f.requests, throwAt > 2 ? ["/version.txt"] : []);
    assert.deepEqual((await readdir(f.out)).sort(), [
      "existing.txt",
      "pkg-v1.zip",
    ]);
    assert.equal(
      await readFile(join(f.out, "pkg-v1.zip"), "utf8"),
      "existing artifact",
    );
    assert.equal(await readFile(join(f.out, "existing.txt"), "utf8"), "keep");
  }
});

test("undeclared Digest header is filtered and cannot supply a checksum", async (t) => {
  const f = await fixture(t, true);
  f.routes["/checksums.txt"] = {
    body: "invalid",
    headers: { Digest: checksum },
  };
  f.routes["/version.txt"].headers = { Digest: checksum };
  assert.deepEqual(
    (await readResponse(`${f.base}/checksums.txt`, { type: "checksum" }))
      .headers,
    [],
  );
  assert.deepEqual(
    (await readResponse(`${f.base}/version.txt`, { type: "text" })).headers,
    [],
  );
  await assert.rejects(
    f.run(["--allow-parsed"]),
    rejectsWith("parser.invalid_checksum"),
  );
  assert.ok(!f.requests.includes("/pkg-v1.zip"));
  assert.deepEqual(await readdir(f.out), []);
});

test("undeclared Digest does not override a valid body checksum", async (t) => {
  const f = await fixture(t, true);
  f.routes["/checksums.txt"].headers = { Digest: `sha256:${"0".repeat(64)}` };
  await f.run(["--allow-parsed"]);
  assert.deepEqual(await readFile(join(f.out, "pkg-v1.zip")), payload);
});

test("declared header parser works and only receives its named header", async (t) => {
  const f = await fixture(t, true);
  assert.equal(f.version.type, "remote");
  const parser: RemoteChecksumParser = {
    type: "header",
    name: "X-Checksum",
    algorithm: "sha256",
    encoding: "hex",
  };
  f.version.checksum!.parser = parser;
  f.routes["/checksums.txt"] = {
    body: "",
    headers: { "X-Checksum": digest, Digest: "ignored" },
  };
  assert.deepEqual(
    (await readResponse(`${f.base}/checksums.txt`, parser)).headers,
    [["x-checksum", digest]],
  );
  await f.run(["--allow-parsed"]);
  assert.deepEqual(await readdir(f.out), ["pkg-v1.zip"]);
});

test("declared header value over 8 KiB fails before splitting without output", async (t) => {
  for (const value of [
    "a".repeat(HEADER_VALUE_LIMIT + 1),
    ["a".repeat(HEADER_VALUE_LIMIT / 2), "a".repeat(HEADER_VALUE_LIMIT / 2)],
  ]) {
    const f = await fixture(t, true);
    assert.equal(f.version.type, "remote");
    f.version.checksum!.parser = {
      type: "header",
      name: "X-Checksum",
      algorithm: "sha256",
      encoding: "hex",
    };
    f.routes["/checksums.txt"].headers = {
      "X-Checksum": value,
    };
    await assert.rejects(
      f.run(["--allow-parsed"]),
      rejectsWith("parser.body_too_large", "response.headers.x-checksum"),
    );
    assert.deepEqual(await readdir(f.out), []);
  }
});

test("identical repeated headers succeed and conflicting occurrences are ambiguous", async (t) => {
  for (const conflicting of [false, true]) {
    const f = await fixture(t, true);
    assert.equal(f.version.type, "remote");
    const parser: RemoteChecksumParser = {
      type: "header",
      name: "X-Checksum",
      algorithm: "sha256",
      encoding: "hex",
    };
    f.version.checksum!.parser = parser;
    const other = conflicting ? "0".repeat(64) : digest;
    f.routes["/checksums.txt"] = {
      headers: { "X-Checksum": digest, "x-checksum": other, Digest: "ignored" },
    };
    assert.deepEqual(
      (await readResponse(`${f.base}/checksums.txt`, parser)).headers,
      [["x-checksum", `${digest}, ${other}`]],
    );
    if (conflicting) {
      await assert.rejects(
        f.run(["--allow-parsed"]),
        rejectsWith("parser.ambiguous", "response.headers.x-checksum"),
      );
      assert.deepEqual(await readdir(f.out), []);
      assert.ok(!f.requests.includes("/pkg-v1.zip"));
    } else {
      await f.run(["--allow-parsed"]);
      assert.deepEqual(await readFile(join(f.out, "pkg-v1.zip")), payload);
    }
  }
});

test("a conflicting header after more than 1000 other headers is still ambiguous", async (t) => {
  const f = await fixture(t, true);
  assert.equal(f.version.type, "remote");
  const parser: RemoteChecksumParser = {
    type: "header",
    name: "X-Checksum",
    algorithm: "sha256",
    encoding: "hex",
  };
  f.version.checksum!.parser = parser;
  const other = "0".repeat(64);
  const headers: Record<string, string> = { "X-Checksum": digest };
  for (let index = 0; index < 1000; index++) headers[`P${index}`] = "";
  headers["x-checksum"] = other;
  f.routes["/checksums.txt"] = { headers };
  assert.deepEqual(
    (await readResponse(`${f.base}/checksums.txt`, parser)).headers,
    [["x-checksum", `${digest}, ${other}`]],
  );
  await assert.rejects(
    f.run(["--allow-parsed"]),
    rejectsWith("parser.ambiguous", "response.headers.x-checksum"),
  );
  assert.deepEqual(await readdir(f.out), []);
  assert.ok(!f.requests.includes("/pkg-v1.zip"));
});

test("five redirects work for artifact, six fail and leave no output", async (t) => {
  const f = await fixture(t);
  f.manifest.registries.main.url_template = `${f.base}/redirect/6/{artifact}`;
  await assert.rejects(f.run(), rejectsWith("http.redirect_limit"));
  assert.deepEqual(await readdir(f.out), []);
  assert.equal(f.requests.length, 6);
  f.manifest.registries.main.url_template = `${f.base}/redirect/5/{artifact}`;
  await f.run();
  assert.deepEqual(await readdir(f.out), ["pkg-v1.zip"]);
});

test("parser body over 2 MiB is rejected for version and checksum responses", async (t) => {
  for (const path of ["/version.txt", "/checksums.txt"]) {
    const f = await fixture(t, true);
    f.routes[path].body = "a".repeat(BODY_LIMIT + 1);
    await assert.rejects(
      f.run(["--allow-parsed"]),
      rejectsWith("parser.body_too_large", "response.body"),
    );
    assert.deepEqual(await readdir(f.out), []);
    assert.ok(!f.requests.includes("/pkg-v1.zip"));
  }
});

test("HTTPS downgrade redirects fail before requesting HTTP for manifest, metadata and artifact", async (t) => {
  for (const kind of ["manifest", "version", "checksum", "artifact"] as const) {
    const f = await fixture(t, kind === "version" || kind === "checksum");
    const secureRequests: string[] = [];
    const sockets = new Set<TLSSocket>();
    const server = createTlsServer({ key: tlsKey, cert: tlsCert }, (socket) => {
      sockets.add(socket);
      socket.on("close", () => sockets.delete(socket));
      socket.once("data", (data) => {
        secureRequests.push(data.toString().split(" ")[1]);
        socket.end(
          `HTTP/1.1 302 Found\r\nLocation: ${f.base}/downgraded\r\nContent-Length: 0\r\nConnection: close\r\n\r\n`,
        );
      });
    });
    await new Promise<void>((resolve) =>
      server.listen(0, "127.0.0.1", resolve),
    );
    t.after(
      () =>
        new Promise<void>((resolve, reject) => {
          server.close((error) => (error ? reject(error) : resolve()));
          for (const socket of sockets) socket.destroy();
        }),
    );
    const address = server.address();
    assert.ok(address && typeof address === "object");
    const secureBase = `https://127.0.0.1:${address.port}`;
    if (kind === "manifest") {
      f.argv[1] = `${secureBase}/manifest.json`;
    } else if (kind === "artifact") {
      f.manifest.registries.main.url_template = `${secureBase}/{artifact}`;
    } else {
      assert.equal(f.version.type, "remote");
      f.manifest.registries.secure = {
        name: "Secure",
        url_template: `${secureBase}/{artifact}`,
      };
      const target = f.version[kind === "version" ? "latest" : kind]!;
      assert.ok("resource" in target);
      target.resource.registry = "secure";
    }
    await f.save();
    const certificatePath = join(f.out, "..", "ca.pem");
    await writeFile(certificatePath, tlsCert);
    await promisify(execFile)(
      process.execPath,
      [
        "--input-type=module",
        "-e",
        `import assert from "node:assert/strict";
       import { run } from ${JSON.stringify(new URL("main.ts", import.meta.url).href)};
       await assert.rejects(run(process.argv.slice(1), { log() {} }), (error) => error.code === "http.insecure_redirect" && error.path === ${JSON.stringify(`http.${kind}`)});`,
        "--",
        ...f.argv,
        "--allow-parsed",
      ],
      { env: { ...process.env, NODE_EXTRA_CA_CERTS: certificatePath } },
    );
    assert.equal(secureRequests.length, 1);
    assert.deepEqual(f.requests, kind === "checksum" ? ["/version.txt"] : []);
    assert.deepEqual(await readdir(f.out), []);
  }
});

test("body limit is measured as UTF-8 bytes with exact limit allowed", async (t) => {
  const f = await fixture(t);
  f.routes["/boundary"] = { body: "é".repeat(BODY_LIMIT / 2) };
  const response = await readResponse(`${f.base}/boundary`);
  assert.equal(Buffer.byteLength(response.body), BODY_LIMIT);
  f.routes["/boundary"].body += "é";
  await assert.rejects(
    readResponse(`${f.base}/boundary`),
    rejectsWith("parser.body_too_large"),
  );
});

test("artifact size cap deletes partial output", async (t) => {
  const f = await fixture(t);
  await assert.rejects(
    f.run(["--max-artifact-bytes", "3"]),
    rejectsWith("download.artifact_too_large"),
  );
  assert.deepEqual(await readdir(f.out), []);
});

test("bodyless artifact responses fail before creating output", async (t) => {
  for (const status of [204, 205]) {
    const f = await fixture(t);
    f.routes["/pkg-v1.zip"] = { status };
    await assert.rejects(
      f.run(),
      rejectsWith("http.empty_body", "http.artifact"),
    );
    assert.deepEqual(await readdir(f.out), []);
  }
});

for (const stall of ["stallHeaders", "stall"] as const) {
  test(
    `timeout closes client sockets for stalled ${stall === "stallHeaders" ? "headers" : "metadata bodies"}`,
    { timeout: 10_000 },
    async (t) => {
      for (const kind of ["manifest", "version", "checksum"] as const) {
        const f = await fixture(t, kind !== "manifest");
        const path = kind === "checksum" ? "/checksums.txt" : `/${kind}.txt`;
        f.routes[path] = { [stall]: true };
        if (kind === "manifest") f.argv[1] = `${f.base}${path}`;
        await assert.rejects(
          f.run(["--allow-parsed"], { timeoutMs: 150 }),
          rejectsWith("http.timeout", `http.${kind}`),
        );
        const closed = f.socketClosures.get(path);
        assert.ok(closed, `${kind} request reached the server`);
        await closed;
        assert.deepEqual(
          f.requests,
          kind === "checksum" ? ["/version.txt", path] : [path],
        );
        assert.deepEqual(await readdir(f.out), []);
      }
    },
  );
}

test(
  "timeout covers a stalled artifact body and removes temporary files",
  { timeout: 10_000 },
  async (t) => {
    const f = await fixture(t);
    f.routes["/pkg-v1.zip"].stall = true;
    await assert.rejects(
      f.run([], { timeoutMs: 150 }),
      rejectsWith("http.timeout"),
    );
    const closed = f.socketClosures.get("/pkg-v1.zip");
    assert.ok(closed);
    await closed;
    assert.deepEqual(await readdir(f.out), []);
  },
);

test(
  "stalled headers and bodies time out and child processes exit naturally",
  { timeout: 20_000 },
  async (t) => {
    for (const stall of ["stallHeaders", "stall"] as const) {
      for (const kind of [
        "manifest",
        "version",
        "checksum",
        "artifact",
      ] as const) {
        const f = await fixture(t, kind === "version" || kind === "checksum");
        const path =
          kind === "artifact"
            ? "/pkg-v1.zip"
            : kind === "checksum"
              ? "/checksums.txt"
              : `/${kind}.txt`;
        f.routes[path] = { [stall]: true };
        if (kind === "manifest") f.argv[1] = `${f.base}${path}`;
        await f.save();
        await promisify(execFile)(
          process.execPath,
          [
            "--input-type=module",
            "-e",
            `import assert from "node:assert/strict";
         import { run } from ${JSON.stringify(new URL("main.ts", import.meta.url).href)};
         await assert.rejects(run(process.argv.slice(1), { log() {}, timeoutMs: 150 }),
           (error) => error.code === "http.timeout" && error.path === ${JSON.stringify(`http.${kind}`)});`,
            "--",
            ...f.argv,
            "--allow-parsed",
          ],
          { timeout: 3_000 },
        );
        assert.ok(f.requests.includes(path));
        assert.deepEqual(await readdir(f.out), []);
      }
    }
  },
);

test("interrupted artifact stream removes temporary files", async (t) => {
  const f = await fixture(t);
  f.routes["/pkg-v1.zip"].disconnect = true;
  await assert.rejects(f.run(), rejectsWith("http.request_failed"));
  assert.deepEqual(await readdir(f.out), []);
});

test("snapshot opt-in uses core snapshot plan only for the exact resolved version", async (t) => {
  const f = await fixture(t, true);
  await f.run(["--allow-snapshot"]);
  assert.deepEqual(f.requests, ["/version.txt", "/pkg-v1.zip"]);
  assert.match(f.logs.join("\n"), /version=v1 kind=snapshot policy=accept/);
  assert.ok(
    f.logs.some(
      (line) => line.startsWith("WARN") && line.includes("--allow-snapshot"),
    ),
  );
});

test("snapshot opt-in cannot use a fallback digest for a newer version", async (t) => {
  const f = await fixture(t, true);
  f.routes["/version.txt"].body = "v2";
  await assert.rejects(
    f.run(["--allow-snapshot"]),
    rejectsWith("resolve.snapshot_unavailable"),
  );
  assert.deepEqual(f.requests, ["/version.txt"]);
  assert.match(f.logs[0], /version=unresolved kind=snapshot policy=accept/);
  assert.ok(!f.logs.some((line) => line.includes("version=v2")));
  assert.deepEqual(await readdir(f.out), []);
});

test("policy logs precede all version, checksum and artifact requests", async (t) => {
  for (const mode of ["static", "parsed", "snapshot", "both"] as const) {
    const f = await fixture(t, mode !== "static");
    const options =
      mode === "static"
        ? []
        : mode === "both"
          ? ["--allow-parsed", "--allow-snapshot"]
          : [`--allow-${mode}`];
    await f.run(options);
    const policies = f.events.filter(
      (event) => event.type === "log" && event.value.includes("policy="),
    );
    for (const event of f.events.filter((event) => event.type === "request")) {
      assert.ok(
        policies.some((policy) => policy.at < event.at),
        `${mode}: policy before ${event.value}`,
      );
      if (event.value !== "/version.txt") {
        assert.ok(
          policies.some(
            (policy) =>
              policy.at < event.at && policy.value.includes("version=v1"),
          ),
          `${mode}: resolved policy before ${event.value}`,
        );
      }
    }
    if (mode !== "static") {
      const kind = mode === "snapshot" ? "snapshot" : "parsed";
      assert.match(
        policies[0].value,
        new RegExp(`version=unresolved kind=${kind} policy=accept`),
      );
      assert.equal(policies.length, 2);
    }
  }
});

test("remote manifest URL and mirrors follow core request plans", async (t) => {
  const f = await fixture(t, true);
  for (const path of ["/version.txt", "/checksums.txt", "/pkg-v1.zip"]) {
    f.routes[`/mirror${path}`] = f.routes[path];
  }
  f.routes["/manifest.json"] = { body: JSON.stringify(f.manifest) };
  f.argv[1] = `${f.base}/manifest.json`;
  await f.run(["--allow-parsed", "--mirror", "proxy"]);
  assert.deepEqual(f.requests, [
    "/manifest.json",
    "/mirror/version.txt",
    "/mirror/checksums.txt",
    "/mirror/pkg-v1.zip",
  ]);
});

test("HTTP status diagnostics use stable locators and redact query strings", async (t) => {
  for (const kind of ["manifest", "version", "checksum", "artifact"] as const) {
    const f = await fixture(t, kind === "version" || kind === "checksum");
    const query = "?token=secret-token&signature=secret-signature";
    const path =
      kind === "artifact"
        ? "/pkg-v1.zip"
        : kind === "checksum"
          ? "/checksums.txt"
          : `/${kind}.txt`;
    if (kind === "manifest") {
      f.argv[1] = `${f.base}${path}${query}`;
    } else {
      f.manifest.registries.main.url_template = `${f.base}/{artifact}${query}`;
      f.routes[`/version.txt${query}`] = f.routes["/version.txt"];
    }
    f.routes[`${path}${query}`] = { status: 503 };
    await assert.rejects(f.run(["--allow-parsed"]), (error) => {
      rejectsWith("http.status", `http.${kind}`)(error);
      assert.equal(
        (error as Error).message,
        `HTTP 503 from ${f.base}${path}?<redacted>`,
      );
      assert.doesNotMatch(
        JSON.stringify(error),
        /secret-token|secret-signature/,
      );
      return true;
    });
    assert.deepEqual(await readdir(f.out), []);
  }
});

test("HTTP timeout and stream errors also redact URLs", async (t) => {
  for (const failure of ["stall", "disconnect"] as const) {
    const f = await fixture(t);
    f.manifest.registries.main.url_template = `${f.base}/{artifact}?token=secret-token`;
    f.routes["/pkg-v1.zip?token=secret-token"] = { [failure]: true };
    await assert.rejects(f.run([], { timeoutMs: 150 }), (error) => {
      rejectsWith(
        failure === "stall" ? "http.timeout" : "http.request_failed",
        "http.artifact",
      )(error);
      assert.match((error as Error).message, /\/pkg-v1\.zip\?<redacted>$/);
      assert.doesNotMatch(JSON.stringify(error), /secret-token/);
      return true;
    });
    assert.deepEqual(await readdir(f.out), []);
  }
});

test("invalid URLs never leak credentials through API or CLI diagnostics", async (t) => {
  const f = await fixture(t);
  f.argv[1] =
    f.base.replace("http://", "http://secret-user:secret-password@") +
    "/manifest.json?token=secret-token#secret-fragment";
  await assert.rejects(f.run(), (error) => {
    rejectsWith("http.invalid_url", "http.manifest")(error);
    assert.doesNotMatch(JSON.stringify(error), /secret-/);
    return true;
  });
  await assert.rejects(
    promisify(execFile)(process.execPath, [
      fileURLToPath(new URL("main.ts", import.meta.url)),
      ...f.argv,
    ]),
    (error: unknown) => {
      const result = error as Error & { code: number; stderr: string };
      assert.equal(result.code, 1);
      assert.match(result.stderr, /http\.invalid_url: .*\[http\.manifest\]/);
      assert.doesNotMatch(result.stderr, /secret-/);
      return true;
    },
  );
  f.routes["/redirect-credentials"] = {
    status: 302,
    headers: { Location: f.argv[1] },
  };
  await assert.rejects(
    readResponse(`${f.base}/redirect-credentials`),
    rejectsWith("http.invalid_url", "http.manifest"),
  );
  assert.deepEqual(f.requests, ["/redirect-credentials"]);
  assert.deepEqual(await readdir(f.out), []);
});

test("variant and tags are mutually exclusive before requests or output", async (t) => {
  const f = await fixture(t);
  await assert.rejects(
    f.run(["--variant", "stable", "--tags", "alpha"]),
    rejectsWith("cli.invalid_args"),
  );
  assert.deepEqual(f.requests, []);
  assert.deepEqual(await readdir(f.out), []);
});

test("Windows reserved device filenames are rejected on every platform before requests", async (t) => {
  const names = [
    "CON",
    "PRN",
    "AUX",
    "NUL",
    ...Array.from({ length: 9 }, (_, i) => `COM${i + 1}`),
    ...Array.from({ length: 9 }, (_, i) => `LPT${i + 1}`),
    ...["¹", "²", "³"].flatMap((suffix) => [`COM${suffix}`, `LPT${suffix}`]),
  ];
  const f = await fixture(t);
  for (const name of names) {
    for (const filename of [
      name,
      name.toLowerCase(),
      `${name.toLowerCase()}.zip`,
      `${name}.tar.gz`,
    ]) {
      f.manifest.resources.sample.variants[0].artifacts[
        "linux-amd64"
      ]!.artifact = filename;
      await assert.rejects(
        f.run(),
        rejectsWith("download.invalid_filename", "artifact"),
      );
      assert.deepEqual(f.requests, []);
      assert.deepEqual(await readdir(f.out), []);
    }
  }
});

test("unsafe final filename segments are rejected before checksum or artifact requests", async (t) => {
  const invalid = [
    "",
    ".",
    "..",
    "nested/",
    "nested/.",
    "nested/..",
    "pkg.zip.",
    "pkg.zip ",
    ...[
      ...'< >:"\\|?*'.replace(" ", ""),
      "\u0000",
      "\u001f",
      "\u007f",
      "\u0085",
    ].map((character) => `pkg${character}.zip`),
  ];
  const f = await fixture(t, true);
  for (const filename of invalid) {
    f.requests.length = 0;
    f.manifest.resources.sample.variants[0].artifacts["linux-amd64"]!.artifact =
      filename;
    await assert.rejects(
      f.run(["--allow-parsed"]),
      rejectsWith("download.invalid_filename", "artifact"),
    );
    assert.deepEqual(f.requests, ["/version.txt"]);
    assert.deepEqual(await readdir(f.out), []);
  }
});

test("safe nested artifact paths keep their final filename", async (t) => {
  const f = await fixture(t);
  for (const filename of [
    "COM0.zip",
    "LPT10.zip",
    "console.zip",
    "pkg-v1.zip",
  ]) {
    f.manifest.resources.sample.variants[0].artifacts["linux-amd64"]!.artifact =
      `nested/${filename}`;
    f.routes[`/nested/${filename}`] = { body: payload };
    assert.equal(await f.run(), join(f.out, filename));
  }
});

test("tags selection is delegated to the core", async (t) => {
  const f = await fixture(t);
  const tag = { id: "alpha", name: "Alpha" };
  f.manifest.resources.sample.tags = [tag];
  Object.assign(f.manifest.resources.sample.variants[0], {
    tag_key: "alpha",
    tags: [tag],
  });
  await f.run(["--tags", "alpha"]);
  assert.deepEqual(await readdir(f.out), ["pkg-v1.zip"]);
});

test("CLI failures exit nonzero and print code, path, message on one error line", async (t) => {
  const f = await fixture(t, true);
  await f.save();
  await assert.rejects(
    promisify(execFile)(process.execPath, [
      fileURLToPath(new URL("main.ts", import.meta.url)),
      ...f.argv,
    ]),
    (error: unknown) => {
      const result = error as Error & { code: number; stderr: string };
      assert.equal(result.code, 1);
      assert.match(
        result.stderr.trim(),
        /^error: verify\.kind_rejected: .*--allow-parsed.*\[arg.kind\]$/,
      );
      return true;
    },
  );
  assert.deepEqual(await readdir(f.out), []);
});
