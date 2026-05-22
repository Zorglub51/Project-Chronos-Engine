# Setup: Ghidra Decompiler on Apple Silicon Mac

Ghidra 12.1_DEV (and likely 11.5+) ships with a native arm64-macOS
decompiler binary, but macOS Gatekeeper blocks it by default because
it's only ad-hoc signed.

## Fix

```sh
GHIDRA=~/dev/ghidra_12.1_DEV
# Strip the quarantine attribute that Gatekeeper checks
xattr -dr com.apple.provenance "$GHIDRA"
xattr -dr com.apple.quarantine "$GHIDRA"
# Re-sign with local adhoc signature so macOS allows execution
codesign --force --sign - "$GHIDRA/Ghidra/Features/Decompiler/os/mac_arm_64/decompile"
codesign --force --sign - "$GHIDRA/Ghidra/Features/Decompiler/os/mac_arm_64/sleigh"
```

## Verify

```sh
spctl --assess --verbose=4 "$GHIDRA/Ghidra/Features/Decompiler/os/mac_arm_64/decompile"
# Should now say "accepted"
```

## Headless decompile example

```sh
mkdir -p ~/ghidra_scripts
cp /path/to/decompile_script.java ~/ghidra_scripts/
"$GHIDRA/support/analyzeHeadless" /path/to/project projectname \
    -process binary -noanalysis \
    -scriptPath ~/ghidra_scripts \
    -postScript decompile_script.java
```

A working script is at `m2hook_vcedump/REPORT.md` Session 10.
