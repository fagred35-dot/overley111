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
..\target\release\overlay-win.exe ..\presets\examples\classic-cross.ovcross.json
..\target\release\overlay-win.exe ..\presets\examples\neon-pulse-ring.ovcross.json --cursor
..\target\release\overlay-win.exe ..\presets\examples\classic-cross.ovcross.json --cursor-blank
```

Хоткеи во время работы: **F7** показать/скрыть (паника), **F8** выход, **F9** переключить центр/курсор.
При выходе в cursor-режиме печатается p50/p95 задержки «опрос мыши → UpdateLayeredWindow».

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

## Известные упрощения (осознанно)

- Рендер CPU (`UpdateLayeredWindow`), не DComp/D2D: для спайка достаточно, сравнение путей — задача M1-3.
- Примитивы только rect/circle/ring/line: проверка окна, а не полноты DSL.
- Нет профилей/авто-детекта игр: режим выбирается флагом, центр берётся из `GetWindowRect(GetForegroundWindow())`.
- Нет анимаций/реактивности: каналы мыши появятся вместе с Expression VM (M6-1).
