# Демо канала управления UI->ядро (M1-1). Оверлей должен быть запущен.
# Примеры (из папки spike):
#   powershell -File tools\ctl-demo.ps1 -PresetFile ..\presets\builtin\neon-pulse-ring.ovcross.json
#   powershell -File tools\ctl-demo.ps1 -Mode cursor
#   powershell -File tools\ctl-demo.ps1 -Quit
param(
  [string]$PresetFile,
  [string]$Mode,
  [switch]$Quit
)
$ErrorActionPreference = "Stop"
if ($PresetFile) {
  $raw = (Get-Content -Raw -Encoding UTF8 $PresetFile).Trim()
  $payload = '{"cmd":"preset","dsl":' + $raw + '}'
} elseif ($Mode) {
  $payload = '{"cmd":"mode","mode":"' + $Mode + '"}'
} elseif ($Quit) {
  $payload = '{"cmd":"quit"}'
} else {
  Write-Host "Нужен один из: -PresetFile <путь к .ovcross.json> | -Mode cursor|center | -Quit"
  exit 1
}
try {
  $c = New-Object System.IO.Pipes.NamedPipeClientStream(".", "overley-ctl", [System.IO.Pipes.PipeDirection]::Out)
  $c.Connect(3000)
  $w = New-Object System.IO.StreamWriter($c)
  $w.WriteLine($payload)
  $w.Flush()
  $c.Close()
  $show = $payload
  if ($show.Length -gt 140) { $show = $show.Substring(0, 140) + "..." }
  Write-Host "[ctl-demo] отправлено: $show"
} catch {
  Write-Host "[ctl-demo] ОШИБКА: оверлей не слушает pipe overley-ctl? ($($_.Exception.Message))"
  exit 2
}
