# CI and release builds

The editor uses the private `pcd-core` converter dependency. A GitHub Actions
token issued to this public repository cannot clone that private repository.
Native editor builds therefore run in `Zorglub51/pce-pcd-rust`, as intended by
the project's private-converter/public-editor arrangement.

## Public checks

`Editor frontend checks` (`.github/workflows/build.yml`) runs the frontend tests
on editor changes and pull requests. It uses `npm ci` and the Node test runner.
Its success reports frontend validation only, not a native compilation.

The old public native-build and tag-release jobs were removed because they
could not authenticate to the converter repository. Do not reintroduce native
builds here without addressing that dependency access explicitly.

`Build Linux recovery` is independent and remains available as a manual workflow.

## Native editor builds

Maintainers with private repository access dispatch these workflows there,
passing the full public editor commit SHA as `editor_ref`:

| Workflow | Output and validation |
| --- | --- |
| `editor-build.yml` | Windows and optional universal macOS builds; Rust and frontend tests; Windows installation and launch |
| `editor-arch.yml` | Native Arch package; tests; installation and welcome-screen rendering |
| `editor-appimage.yml` | Linux AppImage; tests; launch and rendering on Ubuntu and Arch |

These workflows resolve the pinned converter locally in the private checkout.
They are manually dispatched; a public push does not automatically run them.
After successful validation, publish the packaged artifacts and checksums to
the public release, recording the editor revision in the release provenance.

Historical failed runs retain their original result. Rerunning an old commit
uses its old workflow; validate the corrected workflow with a new run instead.
