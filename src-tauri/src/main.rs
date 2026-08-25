// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Every process this binary starts — the window, the standalone viewer, the portal service —
// runs on this allocator. See the note on the `mimalloc` dependency in `Cargo.toml` for why
// the platform default is not used.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    sigma_file_manager_lib::run();
}
