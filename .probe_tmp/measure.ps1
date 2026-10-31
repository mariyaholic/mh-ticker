param([string]$Exe = 'C:\Users\iuniq\Desktop\mariyaholic\mh-ticker\MH_Ticker.exe')
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# One harness, several builds: pass -Exe rather than keeping a variant script per binary.
$exe = $Exe
if (-not (Test-Path -LiteralPath $exe)) { throw "missing: $exe" }
$name = [System.IO.Path]::GetFileNameWithoutExtension($exe)

# Clean up any leftover instance of *this* build from an earlier attempt.
Get-Process -Name $name -ErrorAction SilentlyContinue | ForEach-Object {
    Stop-Process -Id $_.Id -Force
}
Start-Sleep -Seconds 1

$p = Start-Process -FilePath $exe -PassThru
Write-Output ("pid={0}" -f $p.Id)
Start-Sleep -Seconds 4

$p.Refresh()
$cpu0 = $p.CPU
$priv0 = $p.PrivateMemorySize64
$ws0 = $p.WorkingSet64
$threads0 = $p.Threads.Count
Write-Output ("t0  privMB={0:N2} wsMB={1:N2} threads={2} handles={3} cpu={4:N2}" -f `
    ($priv0 / 1MB), ($ws0 / 1MB), $threads0, $p.HandleCount, $cpu0)

$window = 60
Start-Sleep -Seconds $window

$p.Refresh()
$cpu1 = $p.CPU
$priv1 = $p.PrivateMemorySize64
$ws1 = $p.WorkingSet64
Write-Output ("t1  privMB={0:N2} wsMB={1:N2} threads={2} handles={3} cpu={4:N2}" -f `
    ($priv1 / 1MB), ($ws1 / 1MB), $p.Threads.Count, $p.HandleCount, $cpu1)
$cpuPct = (($cpu1 - $cpu0) / $window) * 100.0
Write-Output ("cpu_delta_s={0:N3} over {1}s => {2:N4}% of one core" -f ($cpu1 - $cpu0), $window, $cpuPct)
Write-Output ("priv_delta_MB={0:N3}" -f (($priv1 - $priv0) / 1MB))

Write-Output '--- thread states ---'
$p.Threads | ForEach-Object {
    Write-Output ("  tid={0} state={1} wait={2} cpu={3:N3}" -f $_.Id, $_.ThreadState, $_.WaitReason, $_.TotalProcessorTime.TotalSeconds)
}

Write-Output '--- config written ---'
$cfg = Join-Path $env:LOCALAPPDATA ('MH_Ticker\' + $(if ($name -like '*Ultra*') { 'config_ultra.json' } else { 'config.json' }))
Write-Output ("config_exists={0} path={1}" -f (Test-Path -LiteralPath $cfg), $cfg)

Stop-Process -Id $p.Id -Force
Write-Output 'stopped'
exit 0
