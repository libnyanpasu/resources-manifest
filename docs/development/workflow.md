# Development workflow

## Scope and verification

State assumptions and make material tradeoffs visible before implementation.
Resolve ambiguity before dependent work. Keep changes focused on the request,
match surrounding layout, and remove only dead code created by the change.
Prefer the smallest complete solution while preserving type and crate boundaries.

Plan and implement to the [Rust](rust.md) and [TypeScript](typescript.md) guides.
Name the relevant guide rules in a plan so review can verify them. Existing
violations are migration debt; do not copy or repair them outside the task.

Define observable success criteria. Reproduce bugs and verify fixes; for refactors,
compare behavior before and after. Pair each multi-step task with a check and
report material limitations. Do not merge or push to the base branch while a
required check is red, including a failure on only one platform.

## Repository scripts

Follow [Repository scripts](scripts.md) for layout and named task entrypoints.
Root `package.json` is the task catalog. CI and active documentation should use
`pnpm run`; execution logic belongs in source rather than workflow YAML.

## Worktrees and resource reuse

Prefer isolated Git worktrees for feature/refactoring work. Assess scope,
concurrent edits, installation/build cost, and disk usage before recommending a
worktree or the current checkout. Follow a checkout/branch choice the user has
already made; do not ask again. For a new choice, explain the cost and get the
user's selection before preparing another checkout.

The development drive is `G:` (ReFS). Inspect the actual filesystem and existing
parents before choosing a location. Put Windows ReFS worktrees outside the repo
tree on the same volume, unless the developer chooses otherwise. A matching
filesystem type on different volumes does not prove clone support. Never put build
output on `C:`.

Use existing filesystem capabilities only. Do not provision volumes, VHDs,
filesystems, mount points, or system settings to enable reuse. If CoW support
cannot be verified with available tools, build independently and explain briefly;
do not silently copy a large target or pursue storage workarounds.

| Path                                          | Policy                                                                           |
| --------------------------------------------- | -------------------------------------------------------------------------------- |
| `node_modules/`                               | Independent `pnpm install` in each worktree; pnpm's store handles package reuse. |
| `target/`                                     | Independent files; seed from a verified native CoW clone only when supported.    |
| `packages/manifest-runtime/wasm/`             | Independent generated output; rebuild through `pnpm run build:wasm`.             |
| Generated metadata/runtime source and schemas | Branch-owned files; regenerate and check in that checkout.                       |
| `files/`                                      | Optional read-only reference; not a build or test prerequisite.                  |

### Reuse Cargo target through CoW

- Explain the detected source/destination volumes, reuse method, and whether it
  has been verified for those paths. Recheck if the OS, volumes, or copy tool change.
- Require same-volume ReFS block cloning on Windows. On other hosts, use native
  APFS clones or Btrfs/XFS reflinks only when verified; unsupported or cross-volume
  destinations get an independent target.
- Verify clone support with a disposable file: a clone-only operation or extent
  information must prove sharing, and modifying the destination must leave the
  source intact. A fast copy or successful Robocopy/cp command alone is insufficient.
- Seed only a new, absent target while the source is idle. Do not stop the
  developer's build processes to obtain a seed, overwrite a target, or clean/rebuild
  the main checkout solely to prepare one.
- Prefer the same toolchain, target triple, lockfile, profile, features, and rustflags.
  Preserve timestamps/permissions and avoid links redirecting writes to the source.
- Never share writable targets through symlinks, junctions, cross-worktree hardlinks,
  or a common `CARGO_TARGET_DIR`. CoW shares data blocks, not writable files or locks.
- Treat a clone as a cache, not a completed build. Run the relevant Cargo check;
  do not rewrite fingerprints or absolute paths to force reuse. Rewritten artifacts
  require new space, so directory-size totals alone do not measure CoW savings.

For worktree-preparation tooling changes, test write isolation, source changes,
concurrent builds, and removal without affecting the seed or another worktree.
Verify the unsupported-filesystem path leaves a usable independent checkout.
Fresh worktrees need `pnpm install`; tests build their own ignored wasm glue.

```powershell
git worktree add <worktree-path> -b <type>/<name>
Set-Location <worktree-path>
pnpm install
pnpm run test
```

Remove worktrees with `git worktree remove <worktree-path>`. If Windows long paths
require manual cleanup, first verify the resolved absolute target is the intended
worktree outside the main checkout, use native PowerShell operations on that exact
path, and then run `git worktree prune` and `git worktree list`. Do not remove
another checkout's target or follow a shared writable output link.

## Host toolchain caveat

`G:` has scheduled ReFS ZSTD compression. Compressed toolchains, Cargo registry
sources, or target artifacts can be misread under concurrent cold reads and cause
rustc ICEs or `LNK1190`. Check the relevant files with:

```powershell
fsutil file queryExtentsAndRefCounts <absolute-file-path>
```

All LCN values equal to `0xffffffffffffffff` indicate compressed data. Flush freshly
written files before inspecting extents; otherwise fresh cached writes can give
misleading results. Inspect the actual failing toolchain, registry, or output file,
rather than inferring compression solely from its parent directory.

For this host failure, use a freshly installed toolchain of the required version,
re-extract the affected registry sources, and run `cargo clean` in the affected
checkout before rebuilding. Keep the toolchain, cache, and build output on `G:`;
never relocate build output to `C:`. Do not change global storage policy or clean
another checkout as an incidental workaround. Reduce concurrent reads/build jobs
and distinguish host repair from a source fix. A successful fallback-toolchain
build does not count as validation on the required version.

Use `RUSTUP_TOOLCHAIN=1.95.0`, an empty `CARGO_BUILD_RUSTC_WRAPPER`, and
`CARGO_INCREMENTAL=0` for prescribed verification. These are per-process settings,
not a repository toolchain pin; no `rust-toolchain.toml` exists today.

## Git commits

### Stage only related files

Before committing, run `git status` to review the changes, stage only the files related to this change with explicit paths (`git add <specific-path>`), then verify with `git diff --cached --stat`.

Never use blanket staging such as `git add .`, `-A`, `--all`, or `*`. If something was staged by mistake, unstage it with `git reset HEAD <path>`.

### One commit does one thing

Every commit must be atomic, complete, and buildable.

When the user requests phased delivery, commit each completed, verified phase as
work progresses. Split phases by complete behavior, not by arbitrary file batches;
each commit must build without depending on later phases. Fold corrections into
the unpushed commit they complete, following the rules below.

- One indivisible task is one commit.
- Multiple independent tasks are split into multiple commits.
- Do not commit code you know is broken.
- Do not make fix-up (patch-style) commits on a development branch.

If a commit on a development branch is flawed and has not been pushed, fix it with `git reset --soft HEAD~1` and recommit. If it has already been pushed, any rewrite, amend, or force push requires explicit consent first.

Self-check before committing: does this change complete or correct the previous commit? If yes, fold it into the previous commit with `git reset --soft HEAD~1` and recommit instead of creating a new one. Even when two commits are each individually clean, a later commit that completes an earlier one is still a fix-up commit.

Exploratory work may live on `temp/`, `wip/`, or `scratch/` branches. Do not merge those directly; create a clean branch and reorganize the work into atomic commits.

### Commit message content

The subject states what changed; the body explains why when the problem or the fix is not obvious.

Subject rules:

- Use the imperative mood, stay within 72 characters, and do not end with a period.
- Describe the behavior or capability.

Body rules:

- A non-trivial change must have a body; the body may be omitted only when the subject is fully self-explanatory.
- Explain the root cause and the rationale for the fix: why this is a bug and why this change is needed.
- Do not enumerate changes file by file, and do not restate implementation steps that the diff already shows.
- Describe only the final state relative to the parent commit, not differences between intermediate versions of the same patch (e.g. "v2 fixes X").

### Trust the reader

Assume the reader is a competent developer familiar with the project; do not explain what they already know:

- How to build the project — that belongs in documentation, not in a commit message.
- Obvious statements of usage. Counter-example:

  ```text
  Example usage:
    # use mkv container:
    ffmpeg -hwaccel d3d12va -hwaccel_output_format d3d12 -i input.mp4 -c:v av1_d3d12va output.mkv
  ```

- "Build succeeded" or "all tests green" — the commit's existence already implies it passed.

Mention these only when they are genuinely non-obvious:

- New test commands or tools that do not yet exist in the project.
- Non-standard configuration required to reproduce the result.
- Unusual constraints that affect how the data should be interpreted.

## Stacked pull requests

When work builds on an unmerged branch, make the dependency explicit and target
the preceding branch until it lands. Keep each layer independently reviewable and
buildable. Retarget after the base merges and rerun the relevant checks. Do not
rewrite pushed history without explicit consent. A request to leave changes in the
working tree takes precedence over the commit workflow above.
