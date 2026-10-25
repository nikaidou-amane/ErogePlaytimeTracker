use crate::{database::Database, models::AppSettings, screenshot, tracking::TrackingService};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Sender},
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter};

pub type Bindings = [String; 3];
const VALIDATION_ID: i32 = 100;

pub fn bindings(settings: &AppSettings) -> Bindings {
    [
        settings.screenshot_hotkey.clone(),
        settings.ocr_search_hotkey.clone(),
        settings.tracking_pause_hotkey.clone(),
    ]
}

#[derive(Clone)]
pub struct HotkeyService {
    requests: Sender<Request>,
}
enum Request {
    Check(Bindings, Sender<Result<(), String>>),
    Update(Bindings, Sender<Result<(), String>>),
}

impl HotkeyService {
    pub fn start(
        app: AppHandle,
        db: Database,
        tracker: TrackingService,
        root: PathBuf,
        hotkeys: Bindings,
    ) -> Self {
        let (requests, receiver) = mpsc::channel();
        thread::spawn(move || {
            use windows::Win32::UI::WindowsAndMessaging::*;
            let mut registry = Registry::new(WindowsHotkeys);
            // Register independently at startup so one occupied key does not disable the other.
            for (index, value) in hotkeys.into_iter().enumerate() {
                let mut candidate = registry.current.clone();
                candidate[index] = value;
                if let Err(error) = registry.update(candidate) {
                    log::warn!("hotkey registration failed: {error}");
                }
            }
            let busy = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            loop {
                while let Ok(request) = receiver.try_recv() {
                    let (result, response) = match request {
                        Request::Check(candidate, response) => {
                            (registry.check(&candidate), response)
                        }
                        Request::Update(candidate, response) => {
                            (registry.update(candidate), response)
                        }
                    };
                    let _ = response.send(result.map_err(|e| e.to_string()));
                }
                let mut message = MSG::default();
                while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() } {
                    if message.message == WM_HOTKEY
                        && registry.accepts(message.wParam.0, message.lParam.0)
                    {
                        match message.wParam.0 {
                            1 => {
                                if let Err(e) =
                                    screenshot::capture_focused(&app, &db, &tracker, &root)
                                {
                                    let _ = app.emit(
                                        "screenshot-error",
                                        screenshot::capture_error_message(&e),
                                    );
                                }
                            }
                            2 => crate::game_ocr::start(app.clone(), tracker.clone(), busy.clone()),
                            3 => {
                                if let Err(error) = tracker.toggle_paused() {
                                    log::error!("pause hotkey failed: {error:#}");
                                    let _=app.emit("tracking-pause-error","一時停止状態を変更できませんでした。もう一度お試しください。");
                                }
                            }
                            _ => {}
                        }
                    } else {
                        unsafe {
                            let _ = TranslateMessage(&message);
                            DispatchMessageW(&message);
                        }
                    }
                }
                thread::sleep(Duration::from_millis(25));
            }
        });
        Self { requests }
    }

    pub fn set_hotkeys(&self, hotkeys: Bindings) -> anyhow::Result<()> {
        validate_bindings(&hotkeys)?;
        self.request(|response| Request::Update(hotkeys, response))
    }
    pub fn check_hotkeys(&self, hotkeys: Bindings) -> anyhow::Result<()> {
        validate_bindings(&hotkeys)?;
        self.request(|response| Request::Check(hotkeys, response))
    }
    fn request(
        &self,
        make: impl FnOnce(Sender<Result<(), String>>) -> Request,
    ) -> anyhow::Result<()> {
        let (response, receiver) = mpsc::channel();
        self.requests.send(make(response))?;
        receiver
            .recv_timeout(Duration::from_secs(2))?
            .map_err(anyhow::Error::msg)
    }
}

pub fn validate_bindings(values: &Bindings) -> anyhow::Result<()> {
    for value in values {
        validate_hotkey(value)?;
    }
    for (index, value) in values.iter().enumerate() {
        if value.trim().is_empty() {
            continue;
        }
        for other in &values[index + 1..] {
            anyhow::ensure!(
                other.trim().is_empty() || parse_hotkey(value)? != parse_hotkey(other)?,
                "撮影・OCR検索・一時停止にはそれぞれ別のキーを設定してください。"
            );
        }
    }
    Ok(())
}

trait NativeHotkeys {
    fn register(&mut self, value: &str, id: i32) -> anyhow::Result<()>;
    fn unregister(&mut self, id: i32);
}
struct WindowsHotkeys;
impl NativeHotkeys for WindowsHotkeys {
    fn register(&mut self, value: &str, id: i32) -> anyhow::Result<()> {
        register_hotkey(value, id)
    }
    fn unregister(&mut self, id: i32) {
        unsafe {
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::UnregisterHotKey(None, id);
        }
    }
}
struct Registry<N: NativeHotkeys> {
    native: N,
    current: Bindings,
}
impl<N: NativeHotkeys> Registry<N> {
    fn accepts(&self, id: usize, message: isize) -> bool {
        let Some(value) = id.checked_sub(1).and_then(|index| self.current.get(index)) else {
            return false;
        };
        parse_hotkey(value).is_ok_and(|(modifiers, key)| {
            (modifiers.0 & 0xf) as isize == message & 0xffff
                && key as isize == (message >> 16) & 0xffff
        })
    }
    fn new(native: N) -> Self {
        Self {
            native,
            current: Default::default(),
        }
    }
    fn check(&mut self, candidate: &Bindings) -> anyhow::Result<()> {
        validate_bindings(candidate)?;
        for value in candidate.iter().filter(|v| !v.trim().is_empty()) {
            if self
                .current
                .iter()
                .any(|old| !old.is_empty() && parse_hotkey(old).ok() == parse_hotkey(value).ok())
            {
                continue;
            }
            self.native.register(value, VALIDATION_ID)?;
            self.native.unregister(VALIDATION_ID);
        }
        Ok(())
    }
    fn clear(&mut self) {
        for (index, value) in self.current.iter_mut().enumerate() {
            if !value.is_empty() {
                self.native.unregister(index as i32 + 1);
                value.clear();
            }
        }
    }
    fn install(&mut self, values: Bindings) -> anyhow::Result<()> {
        for (index, value) in values.into_iter().enumerate() {
            if value.trim().is_empty() {
                continue;
            }
            self.native.register(&value, index as i32 + 1)?;
            self.current[index] = value;
        }
        Ok(())
    }
    fn update(&mut self, candidate: Bindings) -> anyhow::Result<()> {
        self.check(&candidate)?;
        if candidate == self.current {
            return Ok(());
        }
        let previous = self.current.clone();
        self.clear();
        if let Err(error) = self.install(candidate) {
            self.clear();
            if self.install(previous).is_err() {
                anyhow::bail!("ショートカットキーを復元できませんでした。設定を確認してください。");
            }
            return Err(error);
        }
        Ok(())
    }
}

#[cfg(windows)]
fn register_hotkey(value: &str, id: i32) -> anyhow::Result<()> {
    let (mods, key) = parse_hotkey(value)?;
    unsafe { windows::Win32::UI::Input::KeyboardAndMouse::RegisterHotKey(None, id, mods, key) }
        .map_err(|_| anyhow::anyhow!("このキーは別のアプリで使用されています"))
}

pub fn validate_hotkey(value: &str) -> anyhow::Result<()> {
    if value.trim().is_empty() {
        return Ok(());
    }
    #[cfg(windows)]
    {
        parse_hotkey(value).map(|_| ())
    }
    #[cfg(not(windows))]
    {
        let _ = value;
        Ok(())
    }
}

#[cfg(windows)]
fn parse_hotkey(
    value: &str,
) -> anyhow::Result<(
    windows::Win32::UI::Input::KeyboardAndMouse::HOT_KEY_MODIFIERS,
    u32,
)> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
    };
    let parts: Vec<_> = value
        .split('+')
        .map(|part| part.trim().to_ascii_uppercase())
        .filter(|part| !part.is_empty())
        .collect();
    anyhow::ensure!(!parts.is_empty(), "ショートカットキーを入力してください");
    let mut mods = MOD_NOREPEAT;
    for modifier in &parts[..parts.len() - 1] {
        mods |= match modifier.as_str() {
            "CTRL" | "CONTROL" => MOD_CONTROL,
            "ALT" => MOD_ALT,
            "SHIFT" => MOD_SHIFT,
            "WIN" | "WINDOWS" => MOD_WIN,
            _ => anyhow::bail!("未対応の修飾キーです: {modifier}"),
        };
    }
    let key_name = parts.last().unwrap();
    let function_key = key_name
        .strip_prefix('F')
        .and_then(|number| number.parse::<u32>().ok())
        .filter(|number| (1..=24).contains(number));
    let key = match key_name.as_str() {
        "PRINTSCREEN" | "PRTSC" => 0x2c,
        "INSERT" => 0x2d,
        "HOME" => 0x24,
        "END" => 0x23,
        "PAGEUP" => 0x21,
        "PAGEDOWN" => 0x22,
        name if name.len() == 1 && name.as_bytes()[0].is_ascii_alphanumeric() => {
            name.as_bytes()[0] as u32
        }
        _ if function_key.is_some() => 0x70 + function_key.unwrap() - 1,
        _ => anyhow::bail!("未対応のキーです: {key_name}"),
    };
    Ok((HOT_KEY_MODIFIERS(mods.0), key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct FakeHotkeys {
        registered: HashMap<i32, String>,
        occupied: Option<String>,
        fail_install: Option<String>,
    }
    impl NativeHotkeys for FakeHotkeys {
        fn register(&mut self, value: &str, id: i32) -> anyhow::Result<()> {
            if self.occupied.as_deref() == Some(value)
                || (id != 3 && self.fail_install.as_deref() == Some(value))
            {
                anyhow::bail!("occupied");
            }
            assert!(!self.registered.contains_key(&id));
            assert!(
                !self
                    .registered
                    .values()
                    .any(|old| parse_hotkey(old).unwrap() == parse_hotkey(value).unwrap())
            );
            self.registered.insert(id, value.into());
            Ok(())
        }
        fn unregister(&mut self, id: i32) {
            self.registered.remove(&id);
        }
    }
    fn pair(first: &str, second: &str) -> Bindings {
        [first.into(), second.into(), String::new()]
    }

    #[test]
    fn three_bindings_validate_without_unbinding_the_pause_key() {
        let mut registry = Registry::new(FakeHotkeys::default());
        let values = ["F9".into(), "F10".into(), "F11".into()];
        registry.update(values.clone()).unwrap();
        registry
            .check(&["Ctrl+F9".into(), "F10".into(), "F11".into()])
            .unwrap();
        assert_eq!(registry.native.registered.get(&3).unwrap(), "F11");
        assert_eq!(registry.native.registered.len(), 3);
        for duplicate in [
            ["F11".into(), "F10".into(), "F11".into()],
            ["".into(), "Ctrl+F11".into(), "control+f11".into()],
        ] {
            assert!(registry.update(duplicate).is_err());
            assert_eq!(registry.current, values);
        }
        registry.native.fail_install = Some("Ctrl+F11".into());
        assert!(
            registry
                .update(["F9".into(), "F10".into(), "Ctrl+F11".into()])
                .is_err()
        );
        assert_eq!(registry.current, values);
    }

    #[test]
    #[ignore = "registers three temporary native global hotkeys"]
    fn native_three_hotkeys_keep_pause_registered_during_validation() {
        let mut registry = Registry::new(WindowsHotkeys);
        let result = (|| -> anyhow::Result<()> {
            registry.update([
                "Ctrl+Alt+Shift+F22".into(),
                "Ctrl+Alt+Shift+F23".into(),
                "Ctrl+Alt+Shift+F24".into(),
            ])?;
            registry.check(&[
                "Ctrl+Alt+Shift+F21".into(),
                "Ctrl+Alt+Shift+F23".into(),
                "Ctrl+Alt+Shift+F24".into(),
            ])?;
            anyhow::ensure!(
                register_hotkey("Ctrl+Alt+Shift+F24", 101).is_err(),
                "pause hotkey was unregistered during validation"
            );
            Ok(())
        })();
        registry.clear();
        unsafe {
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::UnregisterHotKey(None, 101);
        }
        result.unwrap();
    }

    #[test]
    fn rejects_equivalent_keys_but_allows_disabled_bindings() {
        for values in [
            pair("Ctrl+Shift+F10", "shift + control + f10"),
            pair("PrtSc", "PrintScreen"),
        ] {
            assert!(validate_bindings(&values).is_err());
        }
        assert!(validate_bindings(&pair("", "")).is_ok());
        assert!(validate_bindings(&pair("", "F10")).is_ok());
        assert!(validate_bindings(&pair("Ctrl+F10", "F10")).is_ok());
    }

    #[test]
    fn checks_without_changing_registrations_and_supports_swapping() {
        let mut registry = Registry::new(FakeHotkeys::default());
        registry.update(pair("F9", "F10")).unwrap();
        registry.check(&pair("F10", "F9")).unwrap();
        assert_eq!(registry.current, pair("F9", "F10"));
        registry.update(pair("F10", "F9")).unwrap();
        assert_eq!(registry.native.registered.get(&1).unwrap(), "F10");
        assert_eq!(registry.native.registered.get(&2).unwrap(), "F9");
        registry.update(pair("", "")).unwrap();
        assert!(registry.native.registered.is_empty());
    }

    #[test]
    fn restores_both_keys_when_registration_fails_after_preflight() {
        let mut registry = Registry::new(FakeHotkeys::default());
        registry.update(pair("F9", "F10")).unwrap();
        registry.native.fail_install = Some("F11".into());
        assert!(registry.update(pair("Ctrl+F9", "F11")).is_err());
        assert_eq!(registry.current, pair("F9", "F10"));
        assert_eq!(registry.native.registered.len(), 2);
        assert_eq!(registry.native.registered.get(&1).unwrap(), "F9");
        assert_eq!(registry.native.registered.get(&2).unwrap(), "F10");
    }

    #[test]
    fn occupied_keys_do_not_disrupt_either_existing_key() {
        let mut registry = Registry::new(FakeHotkeys::default());
        registry.update(pair("F9", "F10")).unwrap();
        registry.native.occupied = Some("F11".into());
        assert!(registry.update(pair("F11", "F10")).is_err());
        assert_eq!(registry.current, pair("F9", "F10"));
        assert_eq!(registry.native.registered.len(), 2);
    }

    #[test]
    fn ignores_stale_hotkey_messages_after_suspension_or_rebinding() {
        let mut registry = Registry::new(FakeHotkeys::default());
        registry.update(pair("F9", "Ctrl+F10")).unwrap();
        assert!(registry.accepts(1, 0x78 << 16));
        assert!(registry.accepts(2, (0x79 << 16) | 2));
        assert!(!registry.accepts(2, 0x79 << 16));
        assert!(!registry.accepts(3, 0x78 << 16));
        registry.update(pair("F10", "F9")).unwrap();
        assert!(!registry.accepts(1, 0x78 << 16));
        registry.update(pair("", "")).unwrap();
        assert!(!registry.accepts(1, 0x79 << 16));
    }
}
