#[path = "../src/startup.rs"]
mod startup;

use std::ffi::OsString;
use tauri::Manager;

#[test]
fn tauri_keeps_the_restart_environment_during_app_initialization() {
    let mut environment = tauri::Env::default();
    environment.args_os = ["tracker.exe", "--autostart"].map(OsString::from).to_vec();
    let app = tauri::test::mock_builder()
        .manage(startup::restart_environment(environment))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();

    assert_eq!(app.env().args_os, [OsString::from("tracker.exe")]);
}
