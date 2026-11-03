param(
    [string]$Sweep = (Join-Path $PSScriptRoot "..\target\debug\sweep.exe"),
    [string]$Root = (Join-Path $env:TEMP "sweep-crash"),
    [int]$Count = 500,
    [int]$Kills = 50
)

$ErrorActionPreference = "Stop"
$db = Join-Path $Root "crash.db"
$plan = Join-Path $Root "plan.json"
$success = 0
$failures = @()

function Reset-Tree {
    if (Test-Path $Root) { Remove-Item -Recurse -Force $Root }
    New-Item -ItemType Directory -Path (Join-Path $Root "src") -Force | Out-Null
    1..$Count | ForEach-Object {
        $path = Join-Path $Root "src\kit$_.ps1"
        Set-Content -Path $path -Value "Write-Output $_"
        (Get-Item $path).LastWriteTime = (Get-Date).AddDays(-100)
    }
}

for ($i = 1; $i -le $Kills; $i++) {
    Reset-Tree
    & $Sweep --db $db init --root (Join-Path $Root "src") --workspace (Join-Path $Root "work") | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "init failed at $i" }
    & $Sweep --db $db scan | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "scan failed at $i" }
    & $Sweep --db $db plan --lane kits --out $plan | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "plan failed at $i" }

    $env:SWEEP_APPLY_DELAY_MS = "20"
    $proc = Start-Process -FilePath $Sweep -ArgumentList @("--db", $db, "apply", $plan, "--apply") -PassThru -NoNewWindow
    Start-Sleep -Milliseconds (Get-Random -Minimum 200 -Maximum 4000)
    $proc.Refresh()
    if (-not $proc.HasExited) {
        Stop-Process -Id $proc.Id -Force
        $proc.WaitForExit()
    }
    Remove-Item Env:\SWEEP_APPLY_DELAY_MS
    Start-Sleep -Milliseconds 150

    & $Sweep --db $db recover | Out-Null
    if ($LASTEXITCODE -ne 0) {
        $failures += "iteration $i`: recover failed"
        Write-Output "kill $i/$Kills : RECOVER FAILED"
        continue
    }

    $srcFiles = @(Get-ChildItem (Join-Path $Root "src") -File -ErrorAction SilentlyContinue)
    $dstFiles = @(Get-ChildItem (Join-Path $Root "work\kits") -File -ErrorAction SilentlyContinue)
    $all = @($srcFiles) + @($dstFiles)
    $names = @($all | Select-Object -ExpandProperty Name)
    $unique = @($names | Sort-Object -Unique)
    $zero = @($all | Where-Object { $_.Length -eq 0 })
    $dupes = @($names | Group-Object | Where-Object { $_.Count -gt 1 })

    $ok = ($all.Count -eq $Count) -and ($unique.Count -eq $Count) -and ($zero.Count -eq 0) -and ($dupes.Count -eq 0)
    if ($ok) {
        $success++
        Write-Output "kill $i/$Kills : clean"
    } else {
        $failures += "iteration $i`: total=$($all.Count) unique=$($unique.Count) zero=$($zero.Count) dupes=$($dupes.Count)"
        Write-Output "kill $i/$Kills : FAIL total=$($all.Count) unique=$($unique.Count) zero=$($zero.Count) dupes=$($dupes.Count)"
    }
}

Write-Output "RESULT: $success/$Kills clean recoveries"
if ($failures.Count -gt 0) {
    Write-Output "FAILURES:"
    $failures | ForEach-Object { Write-Output "  $_" }
    exit 1
}
