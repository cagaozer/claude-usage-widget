<#
.SYNOPSIS
  Samples ClaudeUsageWidget's memory, handle and GDI counts once a minute to find leaks.
.EXAMPLE
  .\scripts\leakcheck.ps1 -Minutes 180 -Csv leak.csv
  Start the widget first (set "Refresh every" to 30 s for the most pressure).
#>
param(
    [int]$Minutes = 180,
    [string]$Csv = "leakcheck.csv"
)

Add-Type @"
using System; using System.Runtime.InteropServices;
public static class Gui { [DllImport("user32.dll")] public static extern uint GetGuiResources(IntPtr h, uint flags); }
"@

$proc = Get-Process ClaudeUsageWidget -ErrorAction Stop | Select-Object -First 1
"time,private_mb,working_set_mb,handles,gdi,user" | Set-Content $Csv
$rows = @()
for ($i = 0; $i -le $Minutes; $i++) {
    $proc.Refresh()
    if ($proc.HasExited) { Write-Warning "Widget exited after $i minutes"; break }
    $row = [pscustomobject]@{
        time    = (Get-Date).ToString("HH:mm:ss")
        private = [math]::Round($proc.PrivateMemorySize64 / 1MB, 2)
        ws      = [math]::Round($proc.WorkingSet64 / 1MB, 2)
        handles = $proc.HandleCount
        gdi     = [Gui]::GetGuiResources($proc.Handle, 0)
        user    = [Gui]::GetGuiResources($proc.Handle, 1)
    }
    $rows += $row
    "$($row.time),$($row.private),$($row.ws),$($row.handles),$($row.gdi),$($row.user)" | Add-Content $Csv
    if ($i -lt $Minutes) { Start-Sleep -Seconds 60 }
}

# Skip the first 10 minutes of warm-up when judging growth.
$steady = if ($rows.Count -gt 10) { $rows[10..($rows.Count - 1)] } else { $rows }
$first = $steady[0]; $last = $steady[-1]
"Samples: $($rows.Count)  (steady-state from $($first.time) to $($last.time))"
"Private MB : first {0}  last {1}  max {2}" -f $first.private, $last.private, ($steady | Measure-Object private -Maximum).Maximum
"Handles    : first {0}  last {1}  max {2}" -f $first.handles, $last.handles, ($steady | Measure-Object handles -Maximum).Maximum
"GDI objects: first {0}  last {1}  max {2}" -f $first.gdi, $last.gdi, ($steady | Measure-Object gdi -Maximum).Maximum
"USER objs  : first {0}  last {1}  max {2}" -f $first.user, $last.user, ($steady | Measure-Object user -Maximum).Maximum
$growth = $last.private - $first.private
if ([math]::Abs($growth) -lt 1 -and $first.gdi -eq $last.gdi -and $first.user -eq $last.user) { "PASS: flat" } else { "CHECK: private growth $growth MB" }
