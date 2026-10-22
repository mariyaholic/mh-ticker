$ErrorActionPreference = 'Stop'
$exe = $args[0]
$p = Start-Process -FilePath $exe -ArgumentList '--probe' -PassThru -NoNewWindow -RedirectStandardOutput 'D:\mariyaholic-build\probe_out.txt' -RedirectStandardError 'D:\mariyaholic-build\probe_err.txt'
$peak = 0
while (-not $p.HasExited) {
  try { $p.Refresh(); if ($p.PrivateMemorySize64 -gt $peak) { $peak = $p.PrivateMemorySize64 } } catch {}
  Start-Sleep -Milliseconds 40
}
Write-Output ("probe peak private = {0:N2} MB" -f ($peak/1MB))
