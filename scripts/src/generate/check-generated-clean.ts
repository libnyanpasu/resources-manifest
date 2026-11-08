import { execFileSync } from "node:child_process";

const paths = process.argv.slice(2);
if (paths.length === 0) {
  console.error("Usage: pnpm run check:generated");
  process.exit(1);
}

const status = execFileSync(
  "git",
  ["status", "--porcelain", "--untracked-files=all", "--", ...paths],
  { encoding: "utf8" },
);
if (status.length > 0) {
  console.error(
    "Generated files are out of date or untracked; run `pnpm run generate:schema` and commit the result:",
  );
  process.stderr.write(status);
  process.exitCode = 1;
}
