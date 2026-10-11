//! Overley hub: двойной клик по exe → окно хаба (WebView2) + прицел поверх игр.
//! Ядро оверлея живёт в том же процессе, отдельным тредом (overlay_win::win::run_with),
//! правки из UI уходят в него через send_cmd (тот же канал, что pipe overley-ctl).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// Пресет по умолчанию: файл preset.ovcross.json рядом с exe, иначе встроенный крест.
fn default_preset() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let p = exe.parent()?.join("preset.ovcross.json");
    if p.exists() {
        Some(p.to_string_lossy().into_owned())
    } else {
        None
    }
}

#[tauri::command]
fn set_preset(dsl: serde_json::Value) -> Result<String, String> {
    let p = overlay_win::preset::parse_str(&dsl.to_string()).map_err(|e| e.0)?;
    overlay_win::win::send_cmd(overlay_win::win::Cmd::Preset(p));
    Ok("ok".into())
}

#[tauri::command]
fn set_mode(mode: String) -> Result<(), String> {
    let m = match mode.as_str() {
        "cursor" => overlay_win::preset::Mode::Cursor,
        "center" => overlay_win::preset::Mode::Center,
        other => return Err(format!("неизвестный режим: {other}")),
    };
    overlay_win::win::send_cmd(overlay_win::win::Cmd::Mode(m));
    Ok(())
}

#[tauri::command]
fn set_replace(on: bool) {
    overlay_win::win::send_cmd(overlay_win::win::Cmd::Replace(on));
}

#[tauri::command]
fn quit_core() {
    overlay_win::win::send_cmd(overlay_win::win::Cmd::Quit);
}

#[tauri::command]
fn core_alive() -> bool {
    overlay_win::win::core_alive()
}

fn main() {
    // Ядро оверлея: свой тред со своим циклом сообщений (окно создаётся там же).
    std::thread::spawn(|| {
        overlay_win::win::run_with(overlay_win::win::Config {
            preset_path: default_preset(),
            force_mode: None,
            cursor_blank: false,
            scale: 1.0,
            hub: true,
        });
    });

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![set_preset, set_mode, set_replace, quit_core, core_alive])
        .run(tauri::generate_context!())
        .expect("Overley hub: ошибка запуска Tauri");
}
