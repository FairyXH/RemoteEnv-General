$f = "$PSScriptRoot\sosize.txt"
$lines = Get-Content $f
$rows = foreach ($l in $lines) {
  if ($l -match '^\s*(\S+)\s+(\S+)\s+(\S+)\s+(.+?)\s*$') {
    $addr = $Matches[1]
    $size = $Matches[2]
    $type = $Matches[3]
    $name = $Matches[4]
    [pscustomobject]@{ Addr = $addr; Size = $size; Type = $type; Name = $name }
  }
}
$defined = $rows | Where-Object { $_.Type -notin @('U','u','w','v') -and $_.Size -match '^[0-9a-fA-F]+$' }
$parsed = $defined | ForEach-Object {
  $bytes = 0L
  if ($_.Size -match '^[0-9a-fA-F]+$') { $bytes = [Convert]::ToInt64($_.Size, 16) }
  [pscustomobject]@{ Bytes = $bytes; Type = $_.Type; Name = $_.Name }
}
Write-Host "Defined symbols: $($parsed.Count)  Total defined bytes: $([math]::Round((($parsed | Measure-Object Bytes -Sum).Sum)/1MB,2)) MB"

Write-Host ""
Write-Host "--- Top 50 defined symbols by size ---"
$parsed | Sort-Object Bytes -Descending | Select-Object -First 50 | Format-Table -AutoSize @{ N='Size'; E={ "{0,10:N0} B  ({1,8:N2} KB)" -f $_.Bytes, ($_.Bytes/1KB) } }, Type, Name

Write-Host ""
Write-Host "--- Symbols by short name (Rust crate tokens) ---"
$top = $parsed | Sort-Object Bytes -Descending | Select-Object -First 2000
$groups = $top | ForEach-Object {
  $n = $_.Name
  $short = $null
  if ($n -match '_RNvCs[a-zA-Z0-9]+_([0-9]+)([a-z0-9_]+)') { $short = "crate $1$2" }
  elseif ($n -match '_RINv.*?_(?:[0-9]+)?([a-z][a-z0-9_]{2,})') { $short = $Matches[1] }
  elseif ($n -match '^([a-zA-Z_][a-zA-Z0-9_]{4,})') { $short = $Matches[1] }
  if ($short) { [pscustomobject]@{ Crate = $short; Bytes = $_.Bytes } } else { $null }
} | Group-Object Crate | ForEach-Object {
  [pscustomobject]@{ Crate = $_.Name; Bytes = ($_.Group | Measure-Object Bytes -Sum).Sum; Count = $_.Count }
} | Sort-Object Bytes -Descending | Select-Object -First 40
$groups | Format-Table -AutoSize @{ N='Bytes'; E={ "{0,12:N0}  ({1,8:N2} MB)" -f $_.Bytes, ($_.Bytes/1MB) } }, Count, Crate
