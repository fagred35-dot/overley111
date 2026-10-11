//! Ядро оверлея Overley (spike/M1): пресеты DSL v1, растр, win32-окно, канал управления.
//! Bin-обёртка `main.rs` запускает спайк-режим; хаб `spike/hub` — режим Tauri-окна.
pub mod preset;
pub mod raster;

#[cfg(windows)]
pub mod win;
