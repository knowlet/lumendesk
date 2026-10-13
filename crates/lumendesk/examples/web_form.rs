fn main() -> Result<(), lumendesk::DesktopError> {
    lumendesk::webview::run_demo(lumendesk::WindowOptions::new("LumenDesk · Web form"))
}
