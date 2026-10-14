impl<R: tauri::Runtime> crate::desktop_bridge::Page for tauri::Webview<R> {
    fn eval(&self, script: &str) -> Result<(), String> {
        tauri::Webview::eval(self, script).map_err(|_| "The desktop display is unavailable.".into())
    }
}
