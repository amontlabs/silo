pub(crate) struct TauriPage<'a, R: tauri::Runtime>(pub(crate) &'a tauri::Webview<R>);

impl<R: tauri::Runtime> crate::desktop_bridge::Page for TauriPage<'_, R> {
    fn eval(&self, script: &str) -> Result<(), String> {
        self.0
            .eval(script)
            .map_err(|_| "The desktop display is unavailable.".into())
    }
}
