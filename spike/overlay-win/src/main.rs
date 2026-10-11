//! Bin-обёртка спайка: консольный режим замеров M0 (протокол — spike/README.md).
//! Ядро живёт в библиотеке `overlay_win` (src/lib.rs), ею же пользуется хаб spike/hub.

#[cfg(not(windows))]
fn main() {
    eprintln!("Спайк overlay-win собирается и запускается только на Windows.");
    eprintln!("Протокол замеров: spike/README.md. Семантику DSL можно посмотреть без Windows:");
    eprintln!("  python tools/render_preview.py presets/examples/*.json");
    std::process::exit(2);
}

#[cfg(windows)]
fn main() {
    overlay_win::win::run();
}
