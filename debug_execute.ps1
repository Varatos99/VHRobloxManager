# VHRobloxManager Debug Launcher
# Logs console output to debug.log
# Note: Don't set RUST_LOG here - the app filters logs internally
$env:RUST_BACKTRACE="1"

$exePath = ".\target\release\VHRobloxManager.exe"
$logFile = "debug.log"

if (Test-Path $exePath) {
    Write-Host "Starting VHRobloxManager in DEBUG mode..."
    Write-Host "Logs will be written to: $logFile"
    Write-Host ""
    # Use file redirection instead of stream capture
    Start-Process -FilePath $exePath -RedirectStandardOutput $logFile -NoNewWindow -Wait
    Write-Host ""
    Write-Host "Done. Check $logFile for logs."
} else {
    Write-Host "Error: VHRobloxManager.exe not found at $exePath"
    Write-Host "Run 'cargo build --release' first"
}