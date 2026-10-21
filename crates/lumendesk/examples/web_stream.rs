//! Pull-driven typed progress stream. Cancel closes its bounded Rust producer.
use lumendesk::{
    WindowOptions,
    core::{Bridge, BridgeConfig, CoreError, Field, Schema, StreamMethodSchema},
    webview::{APP_ORIGIN, WebApplication, demo_assets},
};
use serde::{Deserialize, Serialize};
use std::time::Duration;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    count: u32,
}
#[derive(Serialize)]
struct Step {
    index: u32,
    label: String,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bridge = Bridge::new(BridgeConfig::production(APP_ORIGIN)?)?;
    bridge.register_stream::<Args, Step, u32, _>(
        StreamMethodSchema::new(
            "progress",
            Schema::object([("count".into(), Field::required(Schema::Integer))]),
            Schema::object([
                ("index".into(), Field::required(Schema::Integer)),
                ("label".into(), Field::required(Schema::String)),
            ]),
            Schema::Integer,
        ),
        |context, args, sink| {
            if args.count > 1000 {
                return Err(CoreError::InvalidArguments(
                    "count must be at most 1000".into(),
                ));
            }
            for index in 1..=args.count {
                context.check_cancelled()?;
                sink.send_wait(Step {
                    index,
                    label: format!("Finished step {index}"),
                })?;
                std::thread::sleep(Duration::from_millis(40));
            }
            Ok(args.count)
        },
    )?;
    if std::env::var_os("LUMENDESK_PRINT_TYPES").is_some() {
        print!("{}", bridge.schemas().typescript());
        return Ok(());
    }
    let html = include_str!("../../../assets/stream.html");
    let mut app = WebApplication::new(bridge);
    app.add_window(
        WindowOptions::new("LumenDesk · Bounded progress stream"),
        demo_assets(html)?,
    )?;
    app.run()?;
    Ok(())
}
