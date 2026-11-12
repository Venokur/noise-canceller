# Освобождает кэш сборки Cargo, не трогая настройки, записи и модели.
#
#   powershell -File scripts\clean.ps1
#   powershell -File scripts\clean.ps1 -DryRun     # только показать
#
# `cargo clean` выбрасывает всё, что соберёт и свежая сборка: следующий
# `npm run tauri build` займёт 5-10 минут. Нужен, когда кэш разъехался (старые
# версии зависимостей, инкрементальные каталоги, установщик на каждый релиз), а
# не между обычными пересборками.
#
# Путь берётся у `cargo metadata`, а не склеивается из `app` и `target`: см.
# scripts\target-dir.ps1.

param([switch]$DryRun)

$ErrorActionPreference = "Stop"
$app = Split-Path $PSScriptRoot -Parent
. (Join-Path $PSScriptRoot "target-dir.ps1")

$target = Get-CargoTargetDir -App $app

function Get-Size([string]$path) {
    if (-not (Test-Path $path)) { return 0 }
    $sum = (Get-ChildItem $path -Recurse -File -Force -ErrorAction SilentlyContinue |
        Measure-Object Length -Sum).Sum
    if (-not $sum) { return 0 }
    return $sum
}

$before = Get-Size $target
Write-Host ("кэш: {0}" -f $target)
Write-Host ("  сейчас {0,7:N2} GB" -f ($before / 1GB))

$installerDir = Join-Path $target "release\bundle\nsis"
$installers = @()
if (Test-Path $installerDir) {
    $installers = @(Get-ChildItem $installerDir -Filter "*-setup.exe" -ErrorAction SilentlyContinue)
}

if ($DryRun) {
    Push-Location $app
    try { & cargo clean --dry-run } finally { Pop-Location }
    # Установщики удаляет сам cargo clean, но в -DryRun их видно отдельно: за
    # релиз их накапливается по 70-80 МБ.
    $installers | ForEach-Object { Write-Host ("  уйдёт {0} ({1:N0} MB)" -f $_.Name, ($_.Length / 1MB)) }
    exit 0
}

Push-Location $app
try { & cargo clean } finally { Pop-Location }

$after = Get-Size $target
Write-Host ("  осталось {0,6:N2} GB (освобождено {1:N2} GB)" -f ($after / 1GB), (($before - $after) / 1GB))
if ($after -gt 0) {
    Write-Host "  в каталоге что-то осталось - вероятно, файл держит запущенный процесс" -ForegroundColor Yellow
}
