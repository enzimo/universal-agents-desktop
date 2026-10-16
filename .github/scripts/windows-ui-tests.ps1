$ErrorActionPreference = 'Stop'
npm.cmd ci --ignore-scripts
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
npm.cmd run test:ui:build
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# The runner's direct process context differs from a normal WebView2 desktop user.
$taskName = ('UA-CI-' + $env:UA_CI_ID + '-UI-') + [guid]::NewGuid().ToString('N')
$root = (Get-Location).Path
$node = (Get-Command node.exe -ErrorAction Stop).Source
$script = Join-Path $root 'tests\ui\launcher.mjs'
$results = Join-Path $root 'target\ui-test-results'
New-Item -ItemType Directory -Force $results | Out-Null
$log = Join-Path $results 'windows-run.log'
$action = New-ScheduledTaskAction -Execute 'cmd.exe' -Argument ('/d /c ""' + $node + '" "' + $script + '" > "' + $log + '" 2>&1"') -WorkingDirectory $root
$principal = New-ScheduledTaskPrincipal -UserId ([System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value) -LogonType Interactive -RunLevel Limited
Register-ScheduledTask -TaskName $taskName -Action $action -Principal $principal | Out-Null
try {
    Start-ScheduledTask -TaskName $taskName
    $completed = $false
    for ($i = 0; $i -lt 180; $i++) {
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
    if (-not $completed) { throw 'UI tests did not finish in the signed-in desktop within 180 seconds.' }
    if ($info.LastTaskResult -ne 0) { throw "UI tests failed with exit code $($info.LastTaskResult)." }
} finally {
    Stop-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
    Unregister-ScheduledTask -TaskName $taskName -Confirm:$false
}
