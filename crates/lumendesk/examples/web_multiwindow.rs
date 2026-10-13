//! Two OS-webview windows, separate page sessions, one typed Rust service/state.
use lumendesk::{
    FormState, WindowOptions,
    core::{Bridge, BridgeConfig, CoreError, Field, MethodSchema, Schema},
    webview::{APP_ORIGIN, WebApplication, demo_assets},
};
use serde::Deserialize;
use std::sync::{Arc, Mutex};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    name: String,
    enthusiastic: bool,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bridge = Bridge::new(BridgeConfig::production(APP_ORIGIN)?)?;
    let mut app = WebApplication::new(bridge.clone());
    for title in ["LumenDesk · First window", "LumenDesk · Second window"] {
        app.add_window(
            WindowOptions::new(title),
            demo_assets(include_str!("../../../assets/index.html"))?,
        )?;
    }
    let fields = Schema::object([
        ("name".into(), Field::required(Schema::String)),
        ("enthusiastic".into(), Field::required(Schema::Boolean)),
        ("greeting".into(), Field::required(Schema::String)),
        ("submissions".into(), Field::required(Schema::Integer)),
    ]);
    app.register_event("greeting", fields.clone())?;
    let events = app.events();
    let state = Arc::new(Mutex::new(FormState::default()));
    bridge.register::<Args, FormState, _>(
        MethodSchema::new(
            "greet",
            Schema::object([
                ("name".into(), Field::required(Schema::String)),
                ("enthusiastic".into(), Field::required(Schema::Boolean)),
            ]),
            fields,
        ),
        move |context, args| {
            context.check_cancelled()?;
            if args.name.len() > 1024 || args.name.trim().is_empty() {
                return Err(CoreError::InvalidArguments(
                    "name must contain 1–1024 bytes".into(),
                ));
            }
            let mut state = state.lock().unwrap();
            state.name = args.name;
            state.enthusiastic = args.enthusiastic;
            state.submit();
            events.emit(None, "greeting", &*state)?;
            Ok(state.clone())
        },
    )?;
    app.run()?;
    Ok(())
}
