# Windows 11 editor

The Windows x64 edition uses the same library format, publisher and CD audio
converter as the Mac edition. No conversion of an existing Chronos library is
needed when moving it between platforms. Select the library's current drive
and folder after moving it; the last-opened path is specific to each computer.

## Install

Run the generated `PCE Game Editor_*_x64-setup.exe` installer. It installs for
the current Windows user and offers English and French installer languages.
It checks Microsoft WebView2 and downloads its bootstrapper if the runtime is
missing. This download requires an Internet connection; importing and editing
local games does not.

Settings and cached BIOS files are kept in the user's application directories,
outside the installed executable. Configure CD BIOS in **Settings**, select
the `.cue` to convert a CD image, or import an existing `.pcd` directly.
Never select just one audio/data `.bin` from a multi-track CD.

## Build locally

Install Rust with the MSVC toolchain, Node.js, Visual Studio C++ Build Tools,
and CMake. Use the **Desktop development with C++** workload. The converter
dependency is private: the Git account used by Cargo must have read access.

```powershell
$env:CARGO_NET_GIT_FETCH_WITH_CLI = 'true'
$env:CMAKE_POLICY_VERSION_MINIMUM = '3.5'
cd editor
npm ci
cargo test --workspace --locked
npm run build -- --ci --bundles nsis -- --locked
```

The CMake setting enables the older bundled Opus build scripts on CMake 4.
The platform-specific `src-tauri/tauri.windows.conf.json` selects NSIS; the Mac
configuration continues to produce an application bundle.

## Automated builds without publishing the converter

Owners can run **Build Chronos editor** in
[`pce-pcd-rust`](https://github.com/Zorglub51/pce-pcd-rust/actions/workflows/editor-build.yml).
Choose the editor commit/tag/branch and optionally include the universal Mac
application. The private job checks out both repositories and makes Cargo
resolve the pinned converter commit through the local private checkout.
No personal access token is copied into this public repository.

The Windows job runs the converter's audio/container tests, the editor's
import/publication/save tests, and the frontend tests. It builds the installer,
installs it into a temporary directory, and checks that the actual application
opens a responsive main window. Its artifact is `chronos-editor-windows-x64`.

The hosted Windows runner is Windows Server 2022. Its automated checks are not
a complete Windows 11 user-session test. Before distributing a release, test
opening a library, importing a CUE/PCD and BIOS, viewing saves, publishing to a
FAT32 USB stick, and restarting the editor to confirm settings persist.

The workflow produces build artifacts; it does not create a public release or
change either repository's visibility. The installer currently has no publisher
certificate. No ROM, BIOS or console dump is included in the artifacts.
