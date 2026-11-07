import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));

function run(command: string, args: string[], list: string): void {
  const result = spawnSync(command, args, {
    cwd: root,
    env: { ...process.env, MANIFEST_CONFORMANCE_LIST: list },
    stdio: "inherit",
  });
  if (result.error || result.status !== 0) {
    throw new Error(
      `${command} ${args.join(" ")} failed: ${result.error?.message ?? (result.signal ? `signal ${result.signal}` : `exit ${result.status}`)}`,
    );
  }
}

function readNames(path: string, side: string): Set<string> {
  const contents = readFileSync(path, "utf8").trimEnd();
  if (!contents) throw new Error(`${side}: empty case list`);
  const names = new Set<string>();
  const duplicates = new Set<string>();
  for (const line of contents.split(/\r?\n/)) {
    const name: unknown = JSON.parse(line);
    if (typeof name !== "string" || name.length === 0) {
      throw new Error(`${side}: invalid case name: ${line}`);
    }
    if (names.has(name)) duplicates.add(name);
    names.add(name);
  }
  if (duplicates.size > 0) {
    throw new Error(
      `${side}: duplicate case names:\n${[...duplicates].sort().join("\n")}`,
    );
  }
  return names;
}

function prefix(name: string): string {
  const [group, subgroup] = name.split(".");
  if (group === "version" || group === "checksum") return `parsers/${group}`;
  if (group === "fixtures") return `${group}/${subgroup}`;
  return group;
}

const temporary = mkdtempSync(join(tmpdir(), "manifest-conformance-"));
try {
  const rustList = join(temporary, "rust.jsonl");
  const nodeList = join(temporary, "node.jsonl");
  writeFileSync(rustList, "");
  writeFileSync(nodeList, "");
  run(
    "cargo",
    ["test", "-p", "manifest-parser", "--test", "conformance"],
    rustList,
  );
  run(
    "cargo",
    [
      "test",
      "-p",
      "manifest-core",
      "--test",
      "conformance",
      "--test",
      "resolve",
      "--test",
      "reference_golden",
    ],
    rustList,
  );
  run(
    process.execPath,
    ["--test", "packages/manifest-runtime/conformance_test.ts"],
    nodeList,
  );
  const rust = readNames(rustList, "Rust");
  const node = readNames(nodeList, "Node");
  const missingRust = [...node].filter((name) => !rust.has(name)).sort();
  const missingNode = [...rust].filter((name) => !node.has(name)).sort();
  if (missingRust.length > 0 || missingNode.length > 0) {
    console.error(`Missing on Rust (${missingRust.length}):`);
    for (const name of missingRust) console.error(name);
    console.error(`Missing on Node (${missingNode.length}):`);
    for (const name of missingNode) console.error(name);
    throw new Error("Rust and Node case lists differ");
  }
  const counts = new Map<string, number>();
  for (const name of rust) {
    const group = prefix(name);
    counts.set(group, (counts.get(group) ?? 0) + 1);
  }
  console.log("Conformance cases by directory/prefix (Rust / Node):");
  for (const [group, count] of [...counts].sort(([a], [b]) =>
    a < b ? -1 : a > b ? 1 : 0,
  )) {
    console.log(`${group}: ${count} / ${count}`);
  }
  console.log(`Total: ${rust.size} / ${node.size}`);
} catch (error) {
  console.error(
    `check:conformance: ${error instanceof Error ? error.message : String(error)}`,
  );
  process.exitCode = 1;
} finally {
  rmSync(temporary, { recursive: true, force: true });
}
