# Releasing (for the maintainer)

Releases are built by GitHub Actions ([`release.yml`](../.github/workflows/release.yml)) for macOS, Windows and Linux, signed for the auto-updater, and published on the Releases page. Installed apps pick them up through `latest.json`.

## Cut a release

```bash
npm run release 0.0.2
git push --follow-tags
```

`npm run release` sets the version everywhere (`package.json`, `tauri.conf.json`, both `Cargo.toml`s), commits, and tags `v0.0.2`. Pushing the tag starts the build. When all three builds finish, the release goes live and apps update.

Edit [`.github/release-notes.md`](../.github/release-notes.md) first if you want to say what's new.

## The update signing key

The auto-updater only installs files signed with the project's private key. The public half lives in `src-tauri/tauri.conf.json`; the private half is in two repository secrets:

| Secret | Value |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | contents of the private key file |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | its password |

**Keep a copy of both somewhere safe** (a password manager). If the key is lost, apps already installed can't verify new updates and everyone has to reinstall by hand.

Never commit the private key.

## Free GitHub Actions minutes

Public repositories get Actions for free. If the repository is ever made private, macOS minutes count 10× against the monthly quota, so each release costs roughly 150–200 minutes.
