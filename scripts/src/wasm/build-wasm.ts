import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { root } from "../shared/repo-paths.ts";

function fail(message: string): never {
  console.error(`build:wasm: ${message}`);
  process.exit(1);
}

function capture(command: string, args: string[]): string {
  const result = spawnSync(command, args, { cwd: root, encoding: "utf8" });
  if (result.error || result.status !== 0) {
    fail(
      `${command} ${args.join(" ")} failed: ${result.error?.message ?? result.stderr.trim()}`,
    );
  }
  return result.stdout.trim();
}

function run(command: string, args: string[]): void {
  const result = spawnSync(command, args, { cwd: root, stdio: "inherit" });
  if (result.error || result.status !== 0) {
    fail(
      `${command} failed${result.error ? `: ${result.error.message}` : ` (exit ${result.status})`}`,
    );
  }
}

const lock = readFileSync(join(root, "Cargo.lock"), "utf8");
const expectedVersion = lock.match(
  /\[\[package\]\]\r?\nname = "wasm-bindgen"\r?\nversion = "([^"]+)"/,
)?.[1];
if (!expectedVersion) {
  fail("Cargo.lock must contain a locked wasm-bindgen version");
}

const cli = spawnSync("wasm-bindgen", ["--version"], {
  cwd: root,
  encoding: "utf8",
});
if (cli.error || cli.status !== 0) {
  fail(
    `wasm-bindgen CLI is missing or unavailable; install it with cargo install wasm-bindgen-cli --version ${expectedVersion} --locked`,
  );
}
const actualVersion = cli.stdout.trim().match(/^wasm-bindgen (\S+)$/)?.[1];
if (actualVersion !== expectedVersion) {
  fail(
    `wasm-bindgen CLI version ${actualVersion ?? JSON.stringify(cli.stdout.trim())} does not match Cargo.lock version ${expectedVersion}; run cargo install wasm-bindgen-cli --version ${expectedVersion} --locked`,
  );
}

const metadata: { target_directory: string } = JSON.parse(
  capture("cargo", [
    "metadata",
    "--format-version",
    "1",
    "--no-deps",
    "--locked",
  ]),
);
run("cargo", [
  "build",
  "-p",
  "manifest-wasm",
  "--target",
  "wasm32-unknown-unknown",
  "--release",
  "--locked",
]);
run("wasm-bindgen", [
  "--target",
  "web",
  "--out-dir",
  join(root, "packages/manifest-runtime/wasm"),
  join(
    metadata.target_directory,
    "wasm32-unknown-unknown/release/manifest_wasm.wasm",
  ),
]);
