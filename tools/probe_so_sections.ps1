$elf = "D:\Files\Develop\Cross-Platform\RemoteEnvCollector\app\desktop\src-tauri\gen\android\app\build\intermediates\merged_native_libs\arm64Debug\mergeArm64DebugNativeLibs\out\lib\arm64-v8a\libremote_env_desktop_lib.so"
$re = "D:\Users\Administrator\Appdata\Local\Android\Sdk\ndk\27.1.12297006\toolchains\llvm\prebuilt\windows-x86_64\bin\llvm-readelf.exe"
$sects = & $re -S $elf 2>$null
$sects | Out-File -Encoding utf8 "$PSScriptRoot\readelf_S.txt"
$rows = @()
foreach ($line in $sects) {
  if ($line -match '\[\s*(\d+)\]\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)') {
    $rows += [pscustomobject]@{
      Idx = $Matches[1]
      Name = $Matches[2]
      Type = $Matches[4]
      Addr = $Matches[5]
      Off = $Matches[6]
      Size = $Matches[7]
      ES = $Matches[8]
      Flags = $Matches[9]
    }
  }
}
Write-Host "Parsed sections: $($rows.Count)"
$parsed = $rows | ForEach-Object {
  $sz = 0L
  if ($_.Size -match '^[0-9a-fA-F]+$') { $sz = [Convert]::ToInt64($_.Size, 16) }
  [pscustomobject]@{ Bytes = $sz; Name = $_.Name; Type = $_.Type; Flags = $_.Flags }
}
Write-Host "--- Top 30 sections by size ---"
$parsed | Sort-Object Bytes -Descending | Select-Object -First 30 | Format-Table -AutoSize @{ N='Size'; E={ "{0,12:N0}  ({1,8:N2} MB)" -f $_.Bytes, ($_.Bytes/1MB) } }, Name, Type, Flags

Write-Host ""
Write-Host "Total file size: $([math]::Round((Get-Item $elf).Length/1MB,2)) MB"

Write-Host ""
Write-Host "--- .debug_* sections ---"
$parsed | Where-Object { $_.Name -match '^(\.debug|\.zdebug)' } | Sort-Object Bytes -Descending | Format-Table -AutoSize @{ N='Size'; E={ "{0,12:N0}  ({1,8:N2} MB)" -f $_.Bytes, ($_.Bytes/1MB) } }, Name
$dbgSum = ($parsed | Where-Object { $_.Name -match '^(\.debug|\.zdebug)' } | Measure-Object Bytes -Sum).Sum
Write-Host "Total .debug*: $([math]::Round($dbgSum/1MB,2)) MB"