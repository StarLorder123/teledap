---
name: git-commit
description: Stage changes and create a conventional git commit with a descriptive message.
---

Generate and execute a git commit based on the current working tree state.

## Steps

1. **Inspect changes** — Run `git status --short` and `git diff --stat` to understand what changed.
2. **Review diffs** — For each modified file, run `git diff -- <file>` to read the actual changes.
3. **Write commit message** — Based on the diffs, generate a conventional commit message:
   - Start with a type prefix: `feat`, `fix`, `refactor`, `docs`, `style`, `test`, `chore`, `perf`.
   - Use imperative mood: `feat: add X`, not `added X` or `adding X`.
   - If changes span multiple concerns, add a body with bullet points.
   - First line under 72 characters.
4. **Update CHANGELOG.md** — Read `CHANGELOG.md` and append a bullet entry under the `## [Unreleased]` section summarizing the changes. Match the conventional commit type:
   - `feat` → `### Added`
   - `fix` → `### Fixed`
   - `refactor` → `### Changed`
   - `docs` → `### Documentation`
   - `chore` / `style` / `test` / `perf` → keep under `[Unreleased]` as a plain bullet.
5. **Stage and commit** — Run `git add -A && git commit -m "<subject>" -m "<body>"`. For multi-line bodies, chain additional `-m` flags (one per paragraph). Never use `@'...'@` or `@"..."@` here-strings — the Bash tool uses POSIX sh, not PowerShell.
6. **Report** — Show the commit hash and summary.

## Rules

- No changes? Say so instead of committing.
- Never commit merge conflicts or broken states unless explicitly asked.
- Ask before `--amend` or `--force-push`.
- Read the diffs, don't guess from filenames.
