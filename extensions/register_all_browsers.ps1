# OTTER 0.1.0 - Register Native Messaging Host across All Installed Windows Browsers
# Supports: Google Chrome, Brave, Microsoft Edge, Opera, Opera GX, Vivaldi, Mozilla Firefox

$manifestPath = Join-Path $PSScriptRoot "chrome\com.otter.download_engine.json"
$resolvedPath = (Resolve-Path $manifestPath).Path

Write-Host "Registering OTTER Native Messaging Host: $resolvedPath" -ForegroundColor Cyan

$regTargets = @(
    "HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.otter.download_engine",
    "HKCU:\Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\com.otter.download_engine",
    "HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\com.otter.download_engine",
    "HKCU:\Software\Opera Software\Opera Stable\NativeMessagingHosts\com.otter.download_engine",
    "HKCU:\Software\Opera Software\Opera GX Stable\NativeMessagingHosts\com.otter.download_engine",
    "HKCU:\Software\Vivaldi\NativeMessagingHosts\com.otter.download_engine"
)

for ($i = 0; $i -lt $regTargets.Count; $i++) {
    $target = $regTargets[$i]
    try {
        $parent = Split-Path -Parent $target
        if (-not (Test-Path $parent)) {
            New-Item -Path $parent -ItemType Directory -Force | Out-Null
        }
        New-Item -Path $target -ItemType Directory -Force | Out-Null
        Set-ItemProperty -Path $target -Name "(Default)" -Value $resolvedPath
        Write-Host " [OK] Registered: $target" -ForegroundColor Green
    } catch {
        Write-Host " [SKIP] $target ($($_.Exception.Message))" -ForegroundColor Yellow
    }
}

$ffManifestPath = Join-Path $PSScriptRoot "firefox\com.otter.download_engine.json"
if (-not (Test-Path $ffManifestPath)) {
    $ffDir = Split-Path -Parent $ffManifestPath
    if (-not (Test-Path $ffDir)) {
        New-Item -Path $ffDir -ItemType Directory -Force | Out-Null
    }
    $ffContent = '{"name":"com.otter.download_engine","description":"OTTER High-Throughput Download Engine Native Messaging Host","path":"otter-browser-host.bat","type":"stdio","allowed_extensions":["otter-engine@otter.dev"]}'
    Set-Content -Path $ffManifestPath -Value $ffContent -Encoding UTF8
}
$resolvedFfPath = (Resolve-Path $ffManifestPath).Path
$ffTarget = "HKCU:\Software\Mozilla\NativeMessagingHosts\com.otter.download_engine"
try {
    $parent = Split-Path -Parent $ffTarget
    if (-not (Test-Path $parent)) {
        New-Item -Path $parent -ItemType Directory -Force | Out-Null
    }
    New-Item -Path $ffTarget -ItemType Directory -Force | Out-Null
    Set-ItemProperty -Path $ffTarget -Name "(Default)" -Value $resolvedFfPath
    Write-Host " [OK] Registered Firefox: $ffTarget" -ForegroundColor Green
} catch {
    Write-Host " [SKIP] Firefox: $ffTarget ($($_.Exception.Message))" -ForegroundColor Yellow
}

Write-Host "All Browser Native Messaging Registrations Completed!" -ForegroundColor Green
