use tauri::Env;

pub fn restart_environment(mut environment: Env) -> Env {
    // This flag describes the current launch only. Tauri forwards this environment
    // to both the updater's installer and process relaunches.
    environment
        .args_os
        .retain(|argument| argument != "--autostart");
    environment
}

#[cfg(test)]
mod tests {
    use super::restart_environment;
    use std::ffi::OsString;

    #[test]
    fn update_and_relaunch_arguments_preserve_the_program_and_other_arguments() {
        let mut environment = tauri::Env::default();
        environment.args_os = ["tracker.exe", "--autostart", "--other", "日本語の引数"]
            .map(OsString::from)
            .to_vec();

        let environment = restart_environment(environment);

        assert_eq!(
            environment.args_os,
            ["tracker.exe", "--other", "日本語の引数"].map(OsString::from)
        );
    }

    #[test]
    fn manual_launch_arguments_are_preserved() {
        let mut environment = tauri::Env::default();
        environment.args_os = ["tracker.exe", "--other"].map(OsString::from).to_vec();
        let arguments = environment.args_os.clone();

        assert_eq!(restart_environment(environment).args_os, arguments);
    }
}
