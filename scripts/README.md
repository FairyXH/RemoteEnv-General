# Build scripts

Run these commands from any directory with PowerShell 7 (`pwsh`):

```powershell
pwsh -File scripts/build-windows.ps1
pwsh -File scripts/build-android.ps1
pwsh -File scripts/build-linux.ps1
pwsh -File scripts/build-macos.ps1
pwsh -File scripts/build-root-module.ps1
pwsh -File scripts/build-all-platforms.ps1
```

On Windows, `scripts\build-all-platforms.cmd` is the double-clickable wrapper
for the same one-click build and forwards any command-line arguments.

All paths are resolved relative to the scripts directory. Outputs are written to
`Release/Windows`, `Release/Android`, `Release/Linux`, and `Release/macOS`.

Desktop packages must be built on their native host. The all-platform script
builds the native desktop target plus Android and the Magisk/KernelSU module,
and reports non-native targets as skipped. Use `-RequireEveryPlatform` in CI to
treat skipped platforms as a failure. Android defaults to a debug APK so it can
be installed without external signing configuration; pass `-ReleaseAndroid`
to the all-platform script, or `-ReleaseApk` to the Android script, for a
release build configured by the local Android signing environment.
