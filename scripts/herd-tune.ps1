<#
.SYNOPSIS
    herd-tune — Detect GPU/VRAM/RAM, configure Ollama, and register with Herd Pro.
.DESCRIPTION
    Run on any Windows machine with Ollama installed to auto-detect hardware,
    apply recommended Ollama environment variables, and register with a Herd Pro instance.
.PARAMETER Apply
    Apply recommended OLLAMA_* environment variables and restart the Ollama service.
    Requires administrator privileges.
.PARAMETER HerdPro
    Override the Herd Pro endpoint URL (default: baked in at download time).
#>
[CmdletBinding()]
param(
    [switch]$Apply,
    [string]$HerdPro
)

# ── Herd Pro Registration (auto-configured on download) ──
$HerdProEndpoint = "%%HERD_PRO_ENDPOINT%%"
$EnrollmentKey = "%%ENROLLMENT_KEY%%"
$HerdTuneVersion = "0.8.0"

# -HerdPro parameter overrides the baked-in endpoint.
# Also check HERD_PRO_URL env var as fallback (useful for containers/CI).
if ($HerdPro) {
    $HerdProEndpoint = $HerdPro
} elseif ($env:HERD_PRO_URL) {
    $HerdProEndpoint = $env:HERD_PRO_URL
}

# ── Require admin only when -Apply is used ──
if ($Apply) {
    $currentPrincipal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
    if (-not $currentPrincipal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        # Re-launch elevated, forwarding arguments
        $argList = "-ExecutionPolicy Bypass -File `"$PSCommandPath`" -Apply"
        if ($HerdProEndpoint -and $HerdProEndpoint -ne '%%HERD_PRO_ENDPOINT%%') {
            $argList += " -HerdPro `"$HerdProEndpoint`""
        }
        Start-Process powershell.exe -Verb RunAs -ArgumentList $argList -Wait
        exit $LASTEXITCODE
    }
}

# ── Detect GPU ──
function Get-GpuInfo {
    # Try nvidia-smi first
    try {
        $smiOutput = & nvidia-smi --query-gpu=name,memory.total --format=csv,noheader,nounits 2>$null
        if ($LASTEXITCODE -eq 0 -and $smiOutput) {
            $parts = $smiOutput.Split(',').Trim()
            return @{ Name = $parts[0]; VramMb = [int]$parts[1] }
        }
    } catch {}

    # WMI fallback
    try {
        $gpu = Get-CimInstance -ClassName Win32_VideoController | Sort-Object AdapterRAM -Descending | Select-Object -First 1
        if ($gpu) {
            $vram = [math]::Round($gpu.AdapterRAM / 1MB)
            return @{ Name = $gpu.Name; VramMb = $vram }
        }
    } catch {}

    return @{ Name = $null; VramMb = 0 }
}

# ── Detect RAM ──
function Get-RamMb {
    try {
        $os = Get-CimInstance -ClassName Win32_OperatingSystem
        return [math]::Round($os.TotalVisibleMemorySize / 1024)
    } catch {
        return 0
    }
}

# ── Detect Ollama ──
function Get-OllamaInfo {
    $ollamaUrl = "http://localhost:11434"

    # Check if Ollama is running
    try {
        $version = Invoke-RestMethod -Uri "$ollamaUrl/api/version" -TimeoutSec 5
        $ollamaVersion = $version.version
    } catch {
        Write-Warning "Ollama is not running at $ollamaUrl"
        return $null
    }

    # Get loaded models
    try {
        $ps = Invoke-RestMethod -Uri "$ollamaUrl/api/ps" -TimeoutSec 5
        $modelsLoaded = @($ps.models | ForEach-Object { $_.name })
    } catch {
        $modelsLoaded = @()
    }

    # Get available models
    try {
        $tags = Invoke-RestMethod -Uri "$ollamaUrl/api/tags" -TimeoutSec 10
        $modelsAvailable = $tags.models.Count
    } catch {
        $modelsAvailable = 0
    }

    # Determine best reachable URL (prefer LAN IP over localhost)
    $bestUrl = $ollamaUrl
    try {
        $adapters = Get-NetIPAddress -AddressFamily IPv4 | Where-Object {
            $_.IPAddress -ne '127.0.0.1' -and $_.PrefixOrigin -ne 'WellKnown'
        } | Sort-Object -Property InterfaceIndex
        if ($adapters) {
            $lanIp = $adapters[0].IPAddress
            $bestUrl = "http://${lanIp}:11434"
        }
    } catch {}

    return @{
        OllamaUrl      = $bestUrl
        OllamaVersion  = $ollamaVersion
        ModelsLoaded   = $modelsLoaded
        ModelsAvailable = $modelsAvailable
    }
}

# ── Calculate recommended config ──
function Get-RecommendedConfig {
    param([int]$VramMb)

    $config = @{
        flash_attention = $true
        kv_cache_type   = "q8_0"
    }

    if ($VramMb -ge 24576) {
        # 24GB+ VRAM (RTX 4090, 5090, A5000, etc.)
        $config.num_parallel    = 8
        $config.max_loaded_models = 4
        $config.max_queue       = 1024
        $config.keep_alive      = "30m"
        $config.context_length  = 16384
    } elseif ($VramMb -ge 12288) {
        # 12-24GB VRAM (RTX 3080, 4070 Ti, etc.)
        $config.num_parallel    = 4
        $config.max_loaded_models = 2
        $config.max_queue       = 512
        $config.keep_alive      = "15m"
        $config.context_length  = 8192
    } elseif ($VramMb -ge 8192) {
        # 8-12GB VRAM (RTX 3070, 4060, etc.)
        $config.num_parallel    = 2
        $config.max_loaded_models = 1
        $config.max_queue       = 256
        $config.keep_alive      = "10m"
        $config.context_length  = 4096
    } else {
        # < 8GB VRAM
        $config.num_parallel    = 1
        $config.max_loaded_models = 1
        $config.max_queue       = 128
        $config.keep_alive      = "5m"
        $config.context_length  = 2048
    }

    return $config
}

# ── Apply Ollama config ──
function Set-OllamaConfig {
    param($Config)

    Write-Host "`n=== Applying Ollama Configuration ===" -ForegroundColor Cyan

    $envVars = @{
        "OLLAMA_NUM_PARALLEL"     = $Config.num_parallel
        "OLLAMA_MAX_LOADED_MODELS" = $Config.max_loaded_models
        "OLLAMA_MAX_QUEUE"        = $Config.max_queue
        "OLLAMA_KEEP_ALIVE"       = $Config.keep_alive
        "OLLAMA_FLASH_ATTENTION"  = if ($Config.flash_attention) { "1" } else { "0" }
        "OLLAMA_KV_CACHE_TYPE"    = $Config.kv_cache_type
        "OLLAMA_CONTEXT_LENGTH"   = $Config.context_length
    }

    foreach ($kv in $envVars.GetEnumerator()) {
        [System.Environment]::SetEnvironmentVariable($kv.Key, [string]$kv.Value, "Machine")
        Write-Host "  Set $($kv.Key) = $($kv.Value)" -ForegroundColor Green
    }

    # Restart Ollama service
    Write-Host "`nRestarting Ollama service..." -ForegroundColor Yellow
    try {
        Restart-Service -Name "OllamaService" -Force -ErrorAction Stop
        Start-Sleep -Seconds 3
        Write-Host "Ollama service restarted." -ForegroundColor Green
    } catch {
        Write-Warning "Could not restart Ollama service automatically. Please restart it manually."
    }
}

# ── Main ──
Write-Host @"

  _               _       _
 | |_  ___ _ _ __| |  ___| |_ _  _ _ _  ___
 | ' \/ -_) '_/ _`` | |___| _| || | ' \/ -_)
 |_||_\___|_| \__,_|     \__|\_,_|_||_\___|

  GPU Detection & Ollama Configuration
  Version $HerdTuneVersion

"@ -ForegroundColor Cyan

# Detect hardware
Write-Host "=== Hardware Detection ===" -ForegroundColor Cyan

$gpu = Get-GpuInfo
$ramMb = Get-RamMb

Write-Host "  GPU:  $($gpu.Name ?? 'Not detected')"
Write-Host "  VRAM: $($gpu.VramMb) MB"
Write-Host "  RAM:  $ramMb MB"

# Detect Ollama
Write-Host "`n=== Ollama Detection ===" -ForegroundColor Cyan
$ollama = Get-OllamaInfo
if (-not $ollama) {
    Write-Error "Ollama is not running. Please start Ollama and try again."
    exit 1
}

Write-Host "  URL:      $($ollama.OllamaUrl)"
Write-Host "  Version:  $($ollama.OllamaVersion)"
Write-Host "  Models:   $($ollama.ModelsAvailable) available, $($ollama.ModelsLoaded.Count) loaded"

# Calculate config
$config = Get-RecommendedConfig -VramMb $gpu.VramMb

Write-Host "`n=== Recommended Configuration ===" -ForegroundColor Cyan
$config.GetEnumerator() | ForEach-Object { Write-Host "  $($_.Key): $($_.Value)" }

$configApplied = $false
if ($Apply) {
    Set-OllamaConfig -Config $config
    $configApplied = $true
} else {
    Write-Host "`nRun with -Apply to set these environment variables and restart Ollama." -ForegroundColor Yellow
}

# Generate stable machine ID (survives hostname changes)
$machineId = $null
try {
    $sid = (Get-CimInstance -ClassName Win32_UserAccount -Filter "LocalAccount=True" -ErrorAction SilentlyContinue |
        Select-Object -First 1).SID
    if ($sid) {
        # Strip the per-user RID to get the machine SID
        $machineSid = $sid -replace '-\d+$'
        $sha = [System.Security.Cryptography.SHA256]::Create()
        $bytes = [System.Text.Encoding]::UTF8.GetBytes($machineSid)
        $hash = $sha.ComputeHash($bytes)
        $machineId = -join ($hash[0..15] | ForEach-Object { $_.ToString("x2") })
    }
} catch {}

# Register with Herd Pro
$regEndpoint = $null
if ($HerdProEndpoint -and $HerdProEndpoint -notmatch 'HERD_PRO_ENDPOINT') {
    $regEndpoint = $HerdProEndpoint
}

if ($regEndpoint) {
    $regUrl = "$regEndpoint/api/nodes/register"
    if ($EnrollmentKey -and $EnrollmentKey -notmatch 'ENROLLMENT_KEY') {
        $regUrl += "?enrollment_key=$EnrollmentKey"
    }

    Write-Host "`n=== Registering with Herd Pro ===" -ForegroundColor Cyan
    Write-Host "  Endpoint: $regEndpoint"

    $payloadObj = @{
        hostname          = $env:COMPUTERNAME.ToLower()
        ollama_url        = $ollama.OllamaUrl
        gpu               = $gpu.Name
        vram_mb           = $gpu.VramMb
        ram_mb            = $ramMb
        ollama_version    = $ollama.OllamaVersion
        models_available  = $ollama.ModelsAvailable
        models_loaded     = $ollama.ModelsLoaded
        recommended_config = $config
        config_applied    = $configApplied
        herd_tune_version = $HerdTuneVersion
        os                = "windows"
        registered_at     = (Get-Date -Format "o")
    }
    if ($machineId) { $payloadObj.node_id = $machineId }
    $payload = $payloadObj | ConvertTo-Json -Depth 3

    try {
        $response = Invoke-RestMethod -Uri $regUrl `
            -Method Post -Body $payload -ContentType "application/json" -TimeoutSec 10
        Write-Host "  Status: $($response.status)" -ForegroundColor Green
        Write-Host "  $($response.message)" -ForegroundColor Green
    } catch {
        Write-Warning "Registration failed: $_"
        Write-Host "  You can register manually later by re-running this script with -HerdPro <url>"
    }
} else {
    Write-Host "`nNo Herd Pro endpoint configured. Run with -HerdPro <url> to register." -ForegroundColor Yellow
}

Write-Host "`nDone!" -ForegroundColor Green
