Add-Type -AssemblyName System.IO.Compression.FileSystem
$apk = "D:\Files\Develop\Cross-Platform\RemoteEnvCollector\Release\Android\app-universal-release-unsigned.apk"
$zip = [System.IO.Compression.ZipFile]::OpenRead($apk)
$so = $zip.Entries | Where-Object { $_.Name -like '*.so' } | Select-Object -First 1
Write-Host ("CompressionMethod: {0}  raw={1:N2}MB  compressed={2:N2}MB" -f $so.CompressionMethod, ($so.Length/1MB), ($so.CompressedLength/1MB))
$zip.Dispose()

Write-Host "--- debug/symbol sections in release .so ---"
$soFile = "D:\Files\Develop\Cross-Platform\RemoteEnvCollector\target\aarch64-linux-android\release\libremote_env_desktop_lib.so"
$re = "D:\Users\Administrator\Appdata\Local\Android\Sdk\ndk\27.1.12297006\toolchains\llvm\prebuilt\windows-x86_64\bin\llvm-readelf.exe"
$sections = & $re -S $soFile 2>$null
$dbg = $sections | Select-String -Pattern "\.debug|\.symtab|\.strtab"
if ($dbg) { $dbg | ForEach-Object { $_.Line.Trim() } } else { Write-Host "No .debug/.symtab/.strtab sections — symbols fully stripped." }
