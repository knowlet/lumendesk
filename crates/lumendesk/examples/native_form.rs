fn main() -> Result<(), lumendesk::DesktopError> {
    lumendesk::native::run_form(lumendesk::WindowOptions::new("LumenDesk · Native form"))
}
