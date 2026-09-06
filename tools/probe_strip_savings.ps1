$elf = "D:\Files\Develop\Cross-Platform\RemoteEnvCollector\app\desktop\src-tauri\gen\android\app\build\intermediates\merged_native_libs\arm64Debug\mergeArm64DebugNativeLibs\out\lib\arm64-v8a\libremote_env_desktop_lib.so"
$strip = "D:\Users\Administrator\Appdata\Local\Android\Sdk\ndk\27.1.12297006\toolchains\llvm\prebuilt\windows-x86_64\bin\llvm-strip.exe"

$orig = (Get-Item $elf).Length
Write-Host ("Original: {0:N2} MB" -f ($orig/1MB))

$bak = "$elf.bak"
Copy-Item $elf $bak -Force

& $strip --strip-debug $elf
$s1 = (Get-Item $elf).Length
Write-Host ("After --strip-debug: {0:N2} MB (saved {1:N2} MB)" -f ($s1/1MB), (($orig-$s1)/1MB))

& $strip --strip-unneeded $elf
$s2 = (Get-Item $elf).Length
Write-Host ("After --strip-unneeded: {0:N2} MB (saved {1:N2} MB)" -f ($s2/1MB), (($s1-$s2)/1MB))

# restore
Move-Item -Force $bak $elf
Write-Host ("Restored: {0:N2} MB" -f ((Get-Item $elf).Length/1MB))