# bench-cube-explorer.ps1
#
# Drives Cube Explorer (cube514htm.exe with WebServer enabled on :8081) over
# its HTTP API for a bench case set, emits CSV in the same schema as
# `cube-cli bench`.
#
# Cube Explorer only supports "solve any state -> solved" cases. DSL / mask /
# impossibility / BaseLimits / MoveSet restriction cases are skipped here and
# left to the cube-core bench. The point of this driver is the **NF1 parity
# baseline**: for cases CE supports, measure CE's wall time as the reference.
#
# Usage:
#   pwsh bench-cube-explorer.ps1 -CasesPath <path> -OutPath <csv> [-Repeat 3] [-Port 8081]
#
# Requires `cube-cli.exe apply-scramble` for facelet generation, since
# applying a scramble to solved is done by cube-core (single source of truth).

param(
    [Parameter(Mandatory=$true)][string]$CasesPath,
    [Parameter(Mandatory=$true)][string]$OutPath,
    [int]$Repeat = 3,
    [int]$Warmup = 1,
    [int]$Port = 8081,
    [string]$CubeCliPath = "",
    [int]$TimeoutSec = 60
)

$ErrorActionPreference = "Stop"

if (-not $CubeCliPath) {
    $CubeCliPath = (Resolve-Path "$PSScriptRoot\..\rust-core\target\release\cube-cli.exe").Path
}
if (-not (Test-Path $CubeCliPath)) {
    Write-Error "cube-cli.exe not found at $CubeCliPath. Build with `cargo build --release --package cube-cli`."
    exit 2
}

# Verify Cube Explorer is reachable.
$baseUrl = "http://127.0.0.1:$Port"
try {
    $r = Invoke-WebRequest -Uri "$baseUrl/?status" -UseBasicParsing -TimeoutSec 5
    if ([System.Text.Encoding]::ASCII.GetString($r.Content) -notmatch "BODY") {
        throw "Unexpected response from $baseUrl/?status"
    }
} catch {
    Write-Error "Cube Explorer WebServer not reachable on $baseUrl. Start cube514htm.exe with cube.ini[WebServer]enabled=1."
    exit 2
}
Write-Host "Cube Explorer reachable on $baseUrl"

# Load cases.
$file = Get-Content $CasesPath -Raw | ConvertFrom-Json
if ($file.version -ne 1) {
    Write-Error "unsupported case file version: $($file.version)"
    exit 2
}

# Pre-compute facelet strings for every case via cube-cli apply-scramble.
# Cube Explorer takes the post-scramble facelet (URFDLB) and returns the
# solving sequence; for OLL/PLL/F2L cases CE will return a FULL solve (to
# solved), not a partial solve, so the time is comparable but the solution
# length is not directly equivalent. We measure time only.
$cases = $file.cases
$rows = New-Object System.Collections.Generic.List[object]
$idx = 0
$total = $cases.Count
foreach ($case in $cases) {
    $idx++
    # CE supports scramble→solved cases. Anything restricted to a subgroup
    # (DSL with <R,U> etc.), impossibility, BaseLimits, or quantifier bounds
    # has no direct CE equivalent; skip those.
    $supported = ($case.category -eq "scramble") -or `
                 ($case.category -eq "oll") -or `
                 ($case.category -eq "pll") -or `
                 ($case.category -eq "f2l")
    if (-not $supported) {
        $row = [pscustomobject]@{
            case_id      = $case.id
            category     = $case.category
            solver       = "cube-explorer"
            runs         = 0
            median_ms    = 0
            p95_ms       = 0
            min_ms       = 0
            max_ms       = 0
            nodes        = 0
            solutions    = 0
            first_length = -1
            status       = "unsupported_by_cube_explorer"
        }
        $rows.Add($row) | Out-Null
        Write-Host "[$idx/$total] $($case.id) ($($case.category))  SKIP (unsupported by CE)"
        continue
    }

    # Build facelet via cube-cli apply-scramble.
    $facelet = & $CubeCliPath apply-scramble $case.scramble
    if ($LASTEXITCODE -ne 0 -or $facelet.Length -lt 54) {
        $row = [pscustomobject]@{
            case_id = $case.id; category = $case.category; solver = "cube-explorer"
            runs = 0; median_ms = 0; p95_ms = 0; min_ms = 0; max_ms = 0
            nodes = 0; solutions = 0; first_length = -1
            status = "error:facelet_gen_failed"
        }
        $rows.Add($row) | Out-Null
        Write-Host "[$idx/$total] $($case.id) ERROR: facelet generation failed"
        continue
    }
    $facelet = $facelet.Trim()

    # Warmup
    for ($w = 0; $w -lt $Warmup; $w++) {
        try { $null = Invoke-WebRequest -Uri "$baseUrl/?$facelet" -UseBasicParsing -TimeoutSec $TimeoutSec } catch {}
    }

    # Timed runs
    $samples = New-Object System.Collections.Generic.List[double]
    $solutionLen = -1
    $timedOut = $false
    for ($r = 0; $r -lt $Repeat; $r++) {
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        try {
            $resp = Invoke-WebRequest -Uri "$baseUrl/?$facelet" -UseBasicParsing -TimeoutSec $TimeoutSec
        } catch {
            $timedOut = $true
            $sw.Stop()
            $samples.Add($sw.Elapsed.TotalMilliseconds) | Out-Null
            break
        }
        $sw.Stop()
        $samples.Add($sw.Elapsed.TotalMilliseconds) | Out-Null
        # Parse solution length from response body.
        $body = [System.Text.Encoding]::ASCII.GetString($resp.Content)
        # Body shape: <HTML><BODY>\n<moves>\n</BODY></HTML>\n
        $m = $body -replace "(?s).*<BODY>\s*", "" -replace "(?s)\s*</BODY>.*", ""
        $m = $m.Trim()
        if ($m -and -not ($m -match "cannot be solved")) {
            $solutionLen = ($m -split "\s+").Count
        } else {
            $solutionLen = 0
        }
    }

    $samples.Sort()
    $median = $samples[[int]([math]::Floor($samples.Count / 2))]
    $p95Idx = [int]([math]::Round(($samples.Count - 1) * 0.95))
    if ($p95Idx -gt $samples.Count - 1) { $p95Idx = $samples.Count - 1 }
    $p95 = $samples[$p95Idx]
    $minv = $samples[0]
    $maxv = $samples[$samples.Count - 1]
    $status = if ($timedOut) { "timeout" } else { "ok" }

    $row = [pscustomobject]@{
        case_id      = $case.id
        category     = $case.category
        solver       = "cube-explorer"
        runs         = $samples.Count
        median_ms    = [int][math]::Round($median)
        p95_ms       = [int][math]::Round($p95)
        min_ms       = [int][math]::Round($minv)
        max_ms       = [int][math]::Round($maxv)
        nodes        = 0  # CE doesn't expose nodes
        solutions    = if ($solutionLen -ge 0) { 1 } else { 0 }
        first_length = $solutionLen
        status       = $status
    }
    $rows.Add($row) | Out-Null
    Write-Host "[$idx/$total] $($case.id) ($($case.category))  median $($row.median_ms) ms  len $solutionLen  status $status"
}

# Write CSV (UTF-8, same schema as cube-cli bench).
$outDir = Split-Path $OutPath -Parent
if ($outDir -and -not (Test-Path $outDir)) {
    New-Item -ItemType Directory -Path $outDir -Force | Out-Null
}
$header = "case_id,category,solver,runs,median_ms,p95_ms,min_ms,max_ms,nodes,solutions,first_length,status"
$lines = @($header)
foreach ($r in $rows) {
    $line = "$($r.case_id),$($r.category),$($r.solver),$($r.runs),$($r.median_ms),$($r.p95_ms),$($r.min_ms),$($r.max_ms),$($r.nodes),$($r.solutions),$($r.first_length),$($r.status)"
    $lines += $line
}
[System.IO.File]::WriteAllLines($OutPath, $lines, [System.Text.UTF8Encoding]::new($false))
Write-Host "wrote $OutPath ($($rows.Count) rows)"
