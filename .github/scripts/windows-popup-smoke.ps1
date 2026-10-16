$ErrorActionPreference = 'Stop'

# Compile in the runner, then exercise WebView2 as the signed-in desktop user.
cargo build -p universal-agents-desktop --example popup_smoke --locked
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$taskName = ('UA-CI-' + $env:UA_CI_ID + '-Popup-') + [guid]::NewGuid().ToString('N')
$root = (Get-Location).Path
$exe = Join-Path $root 'target\debug\examples\popup_smoke.exe'
$log = Join-Path $env:TEMP ($taskName + '.log')
$action = New-ScheduledTaskAction -Execute 'cmd.exe' -Argument ('/d /c ""' + $exe + '" > "' + $log + '" 2>&1"') -WorkingDirectory $root
$principal = New-ScheduledTaskPrincipal -UserId ([System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value) -LogonType Interactive -RunLevel Limited
Register-ScheduledTask -TaskName $taskName -Action $action -Principal $principal | Out-Null
try {
    Start-ScheduledTask -TaskName $taskName
    $completed = $false
    for ($i = 0; $i -lt 120; $i++) {
        Start-Sleep -Seconds 1
        $info = Get-ScheduledTaskInfo -TaskName $taskName
        # Task state and LastTaskResult are separate snapshots. Completion can
        # race this poll, leaving a stale SCHED_S_TASK_RUNNING result.
        if ($info.LastRunTime.Year -gt 2000 -and
            $info.LastTaskResult -notin @(0x41301, 0x41303, 0x41325) -and
            (Get-ScheduledTask -TaskName $taskName).State -eq 'Ready') {
            $completed = $true
            break
        }
    }
    if (Test-Path $log) { Get-Content $log }
    if (-not $completed) { throw 'Popup probe did not finish in the interactive desktop within 120 seconds.' }
    if ($info.LastTaskResult -ne 0) { throw "Popup probe failed with exit code $($info.LastTaskResult)." }
} finally {
    Stop-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
    Unregister-ScheduledTask -TaskName $taskName -Confirm:$false
    Remove-Item $log -ErrorAction SilentlyContinue
}
