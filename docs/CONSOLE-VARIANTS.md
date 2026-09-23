# Japanese and international console resources

The editor supports both retail resource packages. The console model and the
JP/US game lineups are separate choices: each model contains both lineups.

| Console dump | Package directory | JP lineup cover sheet | US lineup cover sheet |
|---|---|---|---|
| PC Engine mini, `1006JP` | `040` | `title_jp_titleselect_jp.psb.m` | `title_jp_titleselect_us.psb.m` |
| CoreGrafx / TurboGrafx-16 mini, `1006WW` | `041` | `title_us_titleselect_jp.psb.m` | `title_us_titleselect_us.psb.m` |

Create a library from the original dump for the desired model. The wizard accepts
an extracted game directory, a raw P9 image (including `mmcblk0p9` without an
extension), a directory of partition images, or a full flash dump. It offers an
empty library or one populated with the original games. Menu resources and BIOS
extraction use the same source in either case.

Detection uses `root.dev_id` in `system/config/system_prof.psb.m`: `40` or `41`.
Do not use `region` (both observed originals say `japan`) or `title_list` (both
list the two models). New libraries preserve this profile in `library/templates/`.
Legacy libraries without it remain usable if their templates contain exactly one
complete resource set. Mixed or incomplete sets produce an error.

Publication preserves the matching title profiles, package-specific menu fields,
cover-sheet structures, filenames and encryption keys. Renaming an encrypted
cover sheet is not a conversion: its basename participates in the MZS key.
The publisher checks that the sibling `game/` runtime matches the template model
before changing published files.

Publishing also installs the updated ARM hook in `game/lib/m2hook_print.so`.
At startup, the hook reads the original `game/version`, validates the matching
resources, and chooses the `040` or `041` bind-mount destinations once. Folder
navigation keeps the existing asynchronous save/mount path. No per-frame model
detection, new console font engine, or Squirrel script changes are needed.
The Linux native test harness uses the same model-specific cover filenames.

## Validation

The supplied `1006JP` P9 and extracted `1006WW` game directory were imported and
published successfully (58 and 57 original game entries respectively). Their
`m2engage` executables have identical SHA-256:
`b02848f66b82f8ac3090db523c4db9633508f9e3f53c7dc0ee3d01ce8aee8792`.
Unit tests cover both identities, legacy templates, missing resources, ambiguous
sets and native hook path selection. Original dumps are read-only inputs and are
not included in the repository or application downloads.

First-boot validation also covers a folder in the first carousel slot. A short
initialization alias keeps long folder identifiers out of the native 15-byte
region field while preserving folder paths, menu tags and save slot numbers.
Fresh system-save metadata is emitted as a PSB v3 with its real size and MD5,
rather than the previous all-zero placeholder rejected by the native loader.
Existing system saves containing user settings remain untouched on publication.
