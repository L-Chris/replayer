# Development preview

- Reuse the fixed Cargo debug executable; do not make a new executable name for
  every build or feature.
- On Windows, use `./scripts/preview.ps1` to rebuild and restart the local preview, or
  `./scripts/preview.ps1 -NoLaunch` when building before Computer Use validation.
- The script closes only `target/debug/pordo.exe` in this checkout (using
  Cargo's actual target directory). Never stop unrelated installed/release apps.
- `-Media "path/to/song.flac"` opens a local media file after rebuilding.
- A failed build must not launch an old executable as though it were current.
- On macOS, use `./scripts/build-macos.sh` and the fixed `target/debug/pordo`
  executable. Source the generated `.deps/macos-<arch>/env.sh` before standalone
  Cargo commands; never use the Windows dependency paths for native macOS builds.
- Keep build outputs, review packages and temporary build caches under this
  checkout's `target/` on F:. Do not move or back up build artifacts to C:.
- Use `./scripts/clean-build.ps1 -DryRun` to inspect disposable local outputs,
  then `./scripts/clean-build.ps1` to remove them while keeping the current preview
  and dependency libraries. Do not rebuild merely to verify cleanup.

# GitHub Release Notes

- The release title must exactly equal the tag, including the lowercase `v`
  prefix (for example, `v0.3.4`). Pass it explicitly as `--title` and verify
  the published title.
- Prepare `docs/releases/<tag>.md` before pushing a release tag. Publish using
  `--notes-file`; do not use automatically generated commit lists as release notes.
- Write English first, followed by a Simplified Chinese translation inside an
  HTML `<details>` block with exactly `<summary>中文更新说明</summary>`. The
  updater uses this marker to select the interface language.
- In both sections, classify entries under `## Feature`, `## Improvement`,
  `## Fix`, in that order; omit empty categories. Keep the same categories,
  item order, authors and links in both languages.
- Write for ordinary users: one short sentence per entry describing the change
  in their experience. Omit internal maintenance without a user-facing effect.
  Assign each change to one primary category, without repeating it elsewhere.
- Describe a new feature and the supporting refinements required to deliver it
  together under Feature. Use Fix for incorrect intended behavior and Improvement
  for an intentional enhancement of already correct behavior.
- Keep English-only content before `<details>` and Chinese-only content inside
  it. If a changelog link is needed in both languages, include it in both sections.
- Before publishing, inspect the commits and associated PRs since the previous
  release, including squash/rebase commit-to-PR associations. Credit only shipped,
  verified external contributions in their individual entries using absolute
  author-profile and PR links (or a verified commit link if there is no PR).
  Attribute PR authors and verified co-authors, not the merger or committer.
  The maintainer is `L-Chris`; omit attribution for the maintainer's own work.
- Write notes explicitly as UTF-8, preferably using apply_patch for Chinese.
  Read the actual file before publication and inspect Chinese text and punctuation;
  reject replacement characters, question marks replacing text, and garbled text.
  After publication, fetch and inspect the release title and body again and compare
  with the local file. Equality alone does not prove the text is correctly encoded.
  Repair damaged text from the intended wording and verify the fetched body again.
- Rebuilding an existing tag must preserve its published release notes.
