# M0 · research-спайк оверлея (Windows, Rust)

Минимальный прототип, который отвечает на вопросы [`docs/M0-decision.md`](../docs/M0-decision.md)
цифрами, а не мнениями. Это **одноразовый код**: в M1 он не переносится как есть, а заменяется
нормальной архитектурой (`windows` crate + DirectComposition/D2D, DSL-компилятор).

> ⚠️ Этот код написан без возможности сборки в среде разработки (нет Windows и нет доступа
> к crates.io). На первой сборке ожидаются правки; места вероятных правок помечены
> комментарием `FIXME-spike`. Если что-то не собирается — правь точечно и отмечай здесь, что поменял.

## Состав

| Файл | Зачем |
| --- | --- |
| `overlay-win/src/main.rs` | Win32-окно оверлея (прямой FFI, без крейта `windows`), цикл сообщений, хоткеи, эксперимент с курсором, замер задержки |
| `overlay-win/src/raster.rs` | CPU-растеризация подмножества DSL (rect/circle/ring/line) в premultiplied BGRA |
| `overlay-win/src/preset.rs` | чтение `.ovcross` (serde), валидация format/version |
| `../tools/render_preview.py` | эталон семантики DSL без Windows: смотри пресеты в PNG до и независимо от спайка |

## Сборка и запуск (только Windows)

Быстрая проверка и починка окружения одной командой:

```powershell
powershell -ExecutionPolicy Bypass -File spike\setup-windows.ps1
```

Вручную:

```powershell
# 1. тулчейн (если нет): rustup → stable-x86_64-pc-windows-msvc
rustup default stable

# 2. сборка
cd spike
cargo build --release
```

### Troubleshooting: `error: linker `link.exe` not found`

Это НЕ ошибка кода спайка. Target `*-pc-windows-msvc` линкуется линкером из Visual Studio C++,
которого нет в «голом» Rust (VS Code линкер не даёт). Два пути:

**Путь 1 (рекомендуется): поставить MSVC Build Tools.** Нужен и позже — с M1 Tauri/WebView2
без MSVC не собираются вовсе, так что ставим сразу:

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools --exact --silent --accept-package-agreements --accept-source-agreements --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
# закрыть терминал, открыть новый
cargo build --release
```

Занимает ~2–4 ГБ и несколько минут; после установки обычный терминал подходит
(rustc сам находит link.exe через vswhere).

**Если winget отвечает «Найден существующий установленный пакет… Доступные обновления не найдены»:**
значит Build Tools уже стоят, но БЕЗ нагрузки C++ — winget не умеет добавлять workload к готовому
продукту и просто ничего не делает. Лечится modify (или GUI: «Visual Studio Installer» →
Build Tools → «Изменить» → галка «Разработка классических приложений на C++» → «Установить»):

```powershell
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$path = & $vswhere -latest -products * -property installationPath | Select-Object -First 1
& "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vs_installer.exe" modify --installPath $path --quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended
```

Проверка диагноза (пустой вывод = компоненты C++ нет):

```powershell
& "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe" -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
```

**Путь 2 (без MSVC): GNU-тулчейн Rust + MinGW-w64.** Только для спайка:

```powershell
rustup toolchain install stable-x86_64-pc-windows-gnu
rustup default stable-x86_64-pc-windows-gnu
winget install MSYS2.MSYS2
C:\msys64\usr\bin\pacman.exe -S --noconfirm mingw-w64-x86_64-gcc
# добавить C:\msys64\mingw64\bin в PATH (или пользоваться "MSYS2 MinGW 64-bit" терминалом)
cargo build --release
```

Если после установки Build Tools ошибка осталась — проверь, что стоишь в том же каталоге
(`spike/`) и что cargo не закешил неудачную сборку: `cargo clean; cargo build --release`.

# 3. запуск поверх игры
.\target\release\overlay-win.exe ..\presets\examples\classic-cross.ovcross.json
.\target\release\overlay-win.exe ..\presets\examples\neon-pulse-ring.ovcross.json --cursor
.\target\release\overlay-win.exe ..\presets\examples\classic-cross.ovcross.json --cursor-blank
```

Хоткеи во время работы: **F7** показать/скрыть (паника, плавно), **F8**/**F12** выход,
**F9** переключить центр/курсор. Появление и уход прицела — плавные (fade 160 мс, smoothstep).
При старте спайк печатает статус каждого хоткея: если комбинация занята другой программой,
будет строка `! хоткей F8 выход НЕ зарегистрирован…`. Запасной выход выбирается из цепочки
**F12 → F10 → Pause/Break → Ctrl+Alt+Q** (первая свободная комбинация); какая досталась тебе —
видно в строке `[spike] хоткеи: F7 показать/скрыть · F8/<твоя> выход · F9 центр/курсор`.
Если заняты все — закрой окно консоли (процесс завершится, курсор восстановится командой
`--restore-cursor`).

> Важно: команды запуска пиши из папки `spike` как `.\target\release\overlay-win.exe …`
> (одна точка). `..\target\…` — это путь на уровень выше, PowerShell его не найдёт.
При выходе в cursor-режиме печатается p50/p95 задержки «опрос мыши → UpdateLayeredWindow».

### Как прогнать режим «за курсором» (`--cursor`)

`--cursor` — это **флаг запуска**, а не кнопка: выйди (F8/F12) и запусти заново с флагом:

```powershell
.\target\release\overlay-win.exe ..\presets\examples\classic-cross.ovcross.json --cursor
```

Либо переключай на лету клавишей **F9** (центр ↔ курсор) в уже запущенном спайке.

### Полное замещение мыши: курсора нет, жив только прицел

```powershell
.\target\release\overlay-win.exe ..\presets\examples\classic-cross.ovcross.json --cursor --cursor-blank
```

`--cursor-blank` подменяет системные курсоры на пустые (и восстанавливает их при выходе
и командой `--restore-cursor`). Работает в любом режиме, не только в center.
Если процесс убили (`taskkill /F`) и курсор пропал: `overlay-win.exe --restore-cursor`.

### Канал управления UI→ядро (M1-1): правь прицел на лету

Оверлей всегда слушает именованный пайп `\\.\pipe\overley-ctl` и принимает JSON-строки
(по одной команде в строке):

| Команда | Пример | Эффект |
| --- | --- | --- |
| `preset` + `dsl` | `{"cmd":"preset","dsl":{…DSL v1…}}` | горячая замена пресета открытого прицела (новый растр, новый DIB, без перезапуска) |
| `preset` + `path` | `{"cmd":"preset","path":"..\\presets\\builtin\\neon-pulse-ring.ovcross.json"}` | то же, но пресет читается из файла |
| `mode` | `{"cmd":"mode","mode":"cursor"}` | центр ↔ курсор |
| `quit` | `{"cmd":"quit"}` | выход (тот же путь, что F8) |

Демо-контроллер (вторая консоль, оверлей уже запущен):

```powershell
powershell -File tools\ctl-demo.ps1 -PresetFile ..\presets\builtin\neon-pulse-ring.ovcross.json
powershell -File tools\ctl-demo.ps1 -Mode cursor
powershell -File tools\ctl-demo.ps1 -Quit
```

Это ровно тот канал, которым в M1-2 воспользуется хаб: exe на Tauri 2 откроет окно с UI
(тот же `ui-prototype/index.html` внутри WebView2) и будет слать сюда правки — изменил цвет
или зазор в редакторе → открытый прицел меняется мгновенно, без перезапуска.

### Размер прицела

Масштаб растра задаёт физический размер: по умолчанию `--scale 1` (холст 128 → окно 128 px).
Крупнее/для HiDPI: `--scale 2`, `--scale 3`. В продукте это будет настройка размера + DPI-масштаб
(DSL `canvas.dpiScale`, PLAN FR-C02/FR-A06).

Если после `--cursor-blank` и аварийного завершения пропал курсор:
`overlay-win.exe --restore-cursor` (или просто запусти спайк снова — он восстановит по lock-файлу на старте M1-версии; в спайке восстановление вручную).

## Протокол замеров

Заполняй таблицы в [`docs/M0-decision.md`](../docs/M0-decision.md). Порядок:

### M0-1 Видимость поверх игры
1. Запусти игру в **Borderless**, дождись матча/полигона.
2. Запусти спайк со статичным пресетом. Оверлей должен быть виден поверх игры.
3. Покликай в игре: стрельба/поворот камеры должны работать (click-through), фокус не должен теряться.
4. Alt+Tab и обратно: оверлей не должен мигать или «проваливаться» под игру.
5. Повтори в **Windowed** и в **Fullscreen (FSE)**: в FSE оверлей, скорее всего, исчезнет — это ожидаемое ограничение, зафиксируй для каждой игры.

### M0-2 Влияние на FPS
```powershell
# CapFrameX или PresentMon; 3 прогона по 60 секунд в одной сцене
PresentMon.exe -process_name <игра> -csv -time 60   # оверлей ВЫКЛ
# затем то же при работающем спайке со статичным пресетом
# затем то же при --cursor (худший случай: перерисовка каждый тик)
```
Сравни avg FPS, 1% low, frame time p95. Цель: ≤ 1% по avg, без деградации 1% low > 3%.

### M0-3 Ресурсы спайка
Диспетчер задач / Process Explorer: working set и CPU спайка в покое (статичный прицел) и в `--cursor`.
Цель покоя: ≤ 15 МБ, CPU ≈ 0 (UpdateLayeredWindow вызывается только при смене позиции в cursor-режиме).

### M0-4 Курсор (самый рискованный эксперимент)
1. `--cursor-blank` → курсор исчез, прицел виден.
2. В другой консоли: `taskkill /F /IM overlay-win.exe`.
3. Проверь: курсор всё ещё не виден? Есть ли `cursor.lock` в `%APPDATA%\Overley-spike\`?
4. `overlay-win.exe --restore-cursor` → курсор вернулся, lock удалён.
5. Повтори 20 раз, зафиксируй `20/20` или меньше. **Если восстановление ненадёжно — режим `system-blank` вычеркиваем из v1.**

### M0-5 Детект FSE
Для 5 игр зафиксируй: режим в настройках игры, реально ли FSE (оверлей не виден), что показывает наша эвристика
(в спайке её нет — используй признаки: оверлей не виден + `GetWindowRect` = весь монитор). Точность заноси в таблицу.

### M0-6 Хоткеи
В матче нажми F7/F8/F9. Долетают ли? В каких играх нет? (Ожидаемый ответ: в части игр с эксклюзивным raw input — нет; тогда в M3 закладываем опцию LL-хука с предупреждением.)

### M0-7 Задержка cursor-режима
Смотри p50/p95 в выводе спайка (мкс). Цель ≤ 4000 мкс (≤ 1 кадр при 240 Гц). Если p95 больше — причина почти всегда в таймере Windows: проверь, что `timeBeginPeriod(1)` применился (таймер должен тикать ~1 мс).

## Критерий «спайк удался»

- Оверлей виден в borderless минимум в 3 играх из 3, клики проходят, фокус не теряется.
- FPS-дельта ≤ 1% (статичный прицел).
- Восстановление курсора 20/20.
- p95 задержки cursor-режима ≤ 4 мс.
- Подписан `docs/M0-decision.md`, старт M1 разрешён.

## Хаб: exe → окно UI + прицел (M1-2, Tauri 2)

`spike/hub` — отдельный крейт: двойной клик по `overley-hub.exe` открывает окно хаба
(WebView2, **не Electron**) и одновременно поднимает прицел поверх окон/игр. Консоли нет.

```powershell
cd spike\hub
cargo build --release          # впервые 5–10 минут: тянется tauri/tao/wry
target\release\overley-hub.exe # или просто двойной клик по exe в проводнике
```

- UI внутри окна — тот же `ui-prototype/index.html` (+ `data.js`), но с мостом: ползунки
  редактора, «Применить к прицелу» в галерее и в ИИ-импорте шлют `set_preset` в ядро —
  открытый прицел меняется мгновенно (FR-G15).
- Пресет по умолчанию: `preset.ovcross.json` рядом с exe, иначе встроенный классический крест.
- Хоткеи ядра работают и из хаба: F7 скрыть/показать, F8 выход ядра (окно хаба остаётся),
  F9 центр/курсор.
- Требуется WebView2 Runtime: на Windows 11 и на Windows 10 с Edge он уже есть; если при
  запуске exe увидишь ошибку про WebView2 — поставь Runtime с сайта Microsoft (evergreen).
- Ошибка «нет цифровой подписи» у `tools\ctl-demo.ps1` — это политика PowerShell, обход:
  `powershell -ExecutionPolicy Bypass -File tools\ctl-demo.ps1 …` (хабу скрипт не нужен).

## Плавность и микро-фризы: что гарантирует спайк, а что приедет в M1

Что есть сейчас (CPU-путь `UpdateLayeredWindow`):

- **center-режим:** перерисовка происходит только когда изменилась геометрия активного окна
  (обычно 0 раз в секунду) → CPU ≈ 0, на FPS игры не влияет.
- **cursor-режим:** событийное обновление по пакетам мыши (**raw input**, `WM_INPUT`,
  `RIDEV_INPUTSINK`) — прицел перерисовывается ровно в момент движения мыши (её репорт-рейт,
  125–1000 Гц), а не поллингом; в простое CPU не тратится. Таймер 250 Гц остаётся только для
  fade и center-геометрии (fallback-поллинг 250 Гц, если raw input не зарегистрировался).
  `UpdateLayeredWindow` маленького BGRA-блока (128–320 px) — доли процента ядра;
  смотри p50/p95 в выводе при выходе (цель ≤ 4000 мкс).
- **Про «отстаёт от мыши»:** собственная задержка ядра ≈ 0,2–0,4 мс (p95); видимая задержка
  складывается из кадра композиции DWM (layered-окно показывается на следующем собранном
  кадре: ~16 мс при 60 Гц, ~7 мс при 144 Гц). Это свойство любого не-инжектного оверлея;
  в M1 DirectComposition flip-model уменьшит её ещё заметнее.
- **fade 160 мс** делается альфой layered-окна (`SourceConstantAlpha`), без пересоздания окна и без
  перерисовки растра — дёшево.
- `timeBeginPeriod(1)` на время работы спайка повышает разрешение системного таймера
  (влияет на энергопотребление ноутбука, не на плавность игры); при выходе возвращается.
- Микро-фризы игры от спайка маловероятны, но проверяются замером, а не верой: PresentMon/CapFrameX,
  3×60 с вкл/выкл (M0-2), цель ≤ 1% по avg и без деградации 1% low.

Чего сейчас НЕТ (приедет в M1, docs/ARCHITECTURE.md §3 и §13):

- DirectComposition + D2D flip-model вместо CPU-блока (меньше копий памяти, present в vsync);
- инвариант «статичный прицел = 0 перерисовок» как формальное требование с тестом в CI;
- перф-бюджеты (≤ 25 МБ ОЗУ, ≤ 1,5% ядра при анимации 60 Гц) как блокирующие регресс-тесты.

## Известные упрощения (осознанно)

- Рендер CPU (`UpdateLayeredWindow`), не DComp/D2D: для спайка достаточно, сравнение путей — задача M1-3.
- Примитивы только rect/circle/ring/line: проверка окна, а не полноты DSL.
- Нет профилей/авто-детекта игр: режим выбирается флагом, центр берётся из `GetWindowRect(GetForegroundWindow())`.
- Нет анимаций/реактивности: каналы мыши появятся вместе с Expression VM (M6-1).
