# Cutting a release

Releases are built by GitHub Actions, never on a developer machine. `.github/workflows/release.yml`
runs on any pushed tag matching `v*`, builds the Linux bundles, and attaches them to a **draft**
release — so nothing is public until you say so.

Everything below assumes the working tree is clean and `main` is pushed.

## The steps

**1. Bump the version.**

Edit `version` in `package.json`, then propagate it to the three Cargo manifests:

```bash
npm run sync-version
```

The workflow refuses to build if `package.json` disagrees with the tag, so this has to happen
before the tag is created, not after.

**2. Check it builds and passes.**

```bash
npm run ts && npm run lint:check && npm run test
cargo check --manifest-path src-tauri/Cargo.toml
```

Do not run a release build locally to "make sure" — see [Local release builds](#local-release-builds).

**3. Commit the bump.**

```bash
git add -A && git commit -m "chore: release X.Y.Z"
```

**4. Tag it.** Annotated, and the tag must be the version with a `v` in front:

```bash
git tag -a vX.Y.Z -m "Sigma File Manager (Cortexist Fork) X.Y.Z

- what changed
- ..."
```

**5. Push, branch first.**

```bash
git push origin main
git push origin vX.Y.Z
```

The tag push is what starts the build. Watch it with:

```bash
gh run watch "$(gh run list --workflow=release.yml --limit 1 --json databaseId --jq '.[0].databaseId')"
```

**6. Write the release notes.** The workflow has already created the draft, titled
`Sigma File Manager (Cortexist Fork) X.Y.Z`, opening with the fork notice and followed by
GitHub's generated list of merged pull requests. Both the title and the body are yours to edit —
the workflow only writes them when it *creates* the release, so hand edits survive a rebuild.

Worth knowing: the generated list only covers work that arrived through pull requests, and it
diffs against the previous `v*` tag it can find. Anything committed straight to `main` will not
appear. The prose you write is where the release is actually described.

**7. Publish.**

```bash
gh release edit vX.Y.Z --draft=false
```

## What comes out

Four assets, each named so the file still identifies itself once it is sitting in somebody's
downloads folder, detached from the page it came from:

| Asset | For |
| --- | --- |
| `Sigma-File-Manager-Cortexist-Fork-X.Y.Z-linux.AppImage` | anyone who wants one file to run |
| `Sigma-File-Manager-Cortexist-Fork-X.Y.Z-linux.deb` | Debian and Ubuntu |
| `Sigma-File-Manager-Cortexist-Fork-X.Y.Z-linux.flatpak` | Flatpak |
| `Sigma-File-Manager-Cortexist-Fork-X.Y.Z-linux-binary` | the bare executable |

**The AppImage name matters.** The in-app updater finds its download by taking the first release
asset whose name ends in `.appimage` (`app_updater.rs`, `pick_release_installer_asset`). Rename
it to something else and auto-update silently stops finding anything.

## Rebuilding a tag that already exists

`release.yml` takes a `tag` input for exactly this:

```bash
gh workflow run release.yml -f tag=vX.Y.Z
```

It reuses the existing draft and replaces the assets, leaving the title and any notes you wrote
alone.

## Things that have bitten us

**A CI fix has to be in the tagged commit.** The build job checks out the *tag*, not `main`, and
`./.github/actions/install-linux-tauri-deps` is resolved from that checkout. Fixing the workflow
on `main` and re-running does nothing. Either move the tag (only reasonable while the release is
an unpublished draft with no assets) or cut the next patch version.

**Versions must stay on their own number line.** They are this fork's, not a continuation of
upstream's, and they are compared by `parse_version_to_number` in `app_updater.rs` — which splits
on `-` and parses each dot-separated segment as an integer. A semver build-metadata form like
`2.2.0+fork.1` parses as plain `2.2.0`, so every release would look like the same version and the
updater would never fire. Anything with a `-` in it is also marked as a prerelease automatically
by the `prepare` job.

**New system dependencies need adding to CI by hand.** The dependency action was inherited from
upstream and knows nothing about what this fork links against. When `Cargo.toml` gains a crate
that binds a system library, add its `-dev` package to
`.github/actions/install-linux-tauri-deps/action.yml` in the same commit. This is how the
GStreamer media player broke the first 2.3.0 build, five minutes in, long after the frontend had
already built.

**A fresh fork will not run workflows at all.** GitHub disables Actions on forked repositories
and silently drops push events until somebody opens the Actions tab and enables them; the dropped
events are never replayed. If a tag push produces no run and the repository has no run history at
all, that is the reason.

<a id="local-release-builds"></a>
**Do not build releases on the workstation.** Release builds have hard-reset this machine (the
8700G's known MCE behavior under sustained load). CI exists partly for this. If you must build
locally, `npm run tauri:build` is the only correct entry point: a bare `cargo build --release`
produces a binary that tries to load the dev server, and AppImage bundling needs both `patchelf`
installed and `NO_STRIP=true`, which that script sets and `npx tauri build` does not.
