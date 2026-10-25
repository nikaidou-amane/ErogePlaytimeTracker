fn main() {
    let notices = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("THIRD_PARTY_LICENSES.txt");
    if !notices.exists() {
        let placeholder = "Eroge Playtime Tracker - Third-Party Licenses\n\nRun `npm run licenses` before packaging to generate the complete dependency notices.\n";
        std::fs::write(&notices, placeholder)
            .unwrap_or_else(|error| panic!("failed to create {}: {error}", notices.display()));
    }
    // Integration tests that construct a Tauri app need the same Common Controls as the app.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-tests=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
    }
    tauri_build::build()
}
