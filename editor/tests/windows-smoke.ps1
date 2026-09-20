# Install the generated package and launch its real executable on a Windows runner.
# All files and processes belong to this smoke test; no user library is opened.
param([Parameter(Mandatory=$true)][string]$Installer)
$ErrorActionPreference = 'Stop'
$installRoot = Join-Path $env:RUNNER_TEMP 'Chronos Windows Test'
$app = $null
try {
    $setup = Start-Process -FilePath $Installer -ArgumentList "/S /D=$installRoot" -Wait -PassThru
    if ($setup.ExitCode -ne 0) { throw "Installer failed: $($setup.ExitCode)" }
    $executable = Join-Path $installRoot 'pce-game-editor.exe'
    if (!(Test-Path -LiteralPath $executable)) { throw "Installed executable is missing: $executable" }
    $app = Start-Process -FilePath $executable -PassThru
    $deadline = (Get-Date).AddSeconds(45)
    do {
        Start-Sleep -Milliseconds 500
        $app.Refresh()
        if ($app.HasExited) { throw "Editor exited during startup: $($app.ExitCode)" }
        if ($app.MainWindowHandle -ne 0 -and $app.Responding -and $app.MainWindowTitle -eq 'PCE Game Editor') {
            Write-Output "Installed editor opened a responsive window (PID $($app.Id))."
            exit 0
        }
    } while ((Get-Date) -lt $deadline)
    throw 'Editor did not open a responsive main window within 45 seconds.'
} finally {
    if ($null -ne $app -and !$app.HasExited) {
        $app.CloseMainWindow() | Out-Null
        if (!$app.WaitForExit(5000)) { Stop-Process -Id $app.Id -Force }
    }
}
