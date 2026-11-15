param([string]$Name = "nocto", [int]$Seconds = 30)
$p = Get-Process -Name $Name -ErrorAction Stop | Select-Object -First 1
$cpu0 = $p.TotalProcessorTime.TotalSeconds
Start-Sleep -Seconds $Seconds
$p.Refresh()
$cpu = (($p.TotalProcessorTime.TotalSeconds - $cpu0) / $Seconds / [Environment]::ProcessorCount) * 100
"WorkingSetMB={0:N1} PrivateMB={1:N1} CpuPercent={2:N2}" -f ($p.WorkingSet64/1MB), ($p.PrivateMemorySize64/1MB), $cpu
