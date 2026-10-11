# Подготовка Windows-машины к сборке спайка Overley (M0).
# Проверяет Rust и MSVC-линкер; если линкера нет — предлагает поставить Build Tools через winget.
#
# Запуск:  powershell -ExecutionPolicy Bypass -File spike\setup-windows.ps1
#
# Почему MSVC, а не MinGW: спайк ещё можно собрать GNU-тулчейном (см. README),
# но начиная с M1 (Tauri/WebView2) MSVC Build Tools обязательны — ставим их сразу.

$ErrorActionPreference = 'Stop'

function Has-Command($name) { return [bool](Get-Command $name -ErrorAction SilentlyContinue) }

Write-Host '== Overley spike: проверка окружения ==' -ForegroundColor Cyan

if (-not (Has-Command 'cargo')) {
    Write-Host 'НЕ НАЙДЕН cargo.' -ForegroundColor Yellow
    Write-Host 'Установи Rust:   winget install Rustlang.Rustup'
    Write-Host 'После установки закрой терминал и открой заново.'
    exit 2
}
cargo --version

# link.exe из обычного терминала не виден; rustc сам находит его через vswhere/реестр,
# поэтому проверяем наличие компонента VC.Tools именно через vswhere.
$installerDir = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer"
$vswhere = Join-Path $installerDir 'vswhere.exe'
$installPath = $null
$hasVcTools = $false
if (Test-Path $vswhere) {
    $installPath = (& $vswhere -latest -products * -property installationPath 2>$null | Select-Object -First 1)
    $hasVcTools = [bool](& $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null)
}

if ($installPath -and $hasVcTools) {
    Write-Host "MSVC C++ найден: $installPath" -ForegroundColor Green
}
elseif ($installPath) {
    # Продукт VS/Build Tools установлен, НО без workload C++ — типичный случай, когда
    # winget говорит «пакет уже установлен, обновлений нет», а link.exe всё равно нет.
    Write-Host ''
    Write-Host "Найден установленный Visual Studio-продукт без компоненты C++:" -ForegroundColor Yellow
    Write-Host "  $installPath"
    Write-Host 'Добавляю workload "Desktop development with C++" (modify)...' -ForegroundColor Yellow
    & (Join-Path $installerDir 'vs_installer.exe') modify --installPath $installPath --quiet --wait `
        --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended
    Write-Host ''
    Write-Host 'Готово. ЗАКРОЙ терминал, открой новый и повтори: cd spike; cargo build --release' -ForegroundColor Green
    exit 0
}
else {
    Write-Host ''
    Write-Host 'MSVC-линкер (link.exe) не найден: rustc для target *-pc-windows-msvc без него не соберёт бинарник.' -ForegroundColor Yellow
    Write-Host 'Ошибка в этом случае выглядит так: error: linker `link.exe` not found' -ForegroundColor DarkGray
    $answer = Read-Host 'Поставить Build Tools for Visual Studio 2022 (C++) через winget? [y/N]'
    if ($answer -ne 'y') {
        Write-Host 'Тогда вариант без MSVC: GNU-тулчейн (spike/README.md, раздел "link.exe not found").'
        exit 3
    }
    winget install --id Microsoft.VisualStudio.2022.BuildTools --exact --silent `
        --accept-package-agreements --accept-source-agreements `
        --override '--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended'
    Write-Host ''
    Write-Host 'Установлено. ЗАКРОЙ терминал, открой новый и повтори: cargo build --release' -ForegroundColor Green
    exit 0
}

Write-Host 'Окружение готово. Собирай:' -ForegroundColor Green
Write-Host '  cd spike; cargo build --release'
Write-Host '  ..\target\release\overlay-win.exe ..\presets\examples\classic-cross.ovcross.json'
