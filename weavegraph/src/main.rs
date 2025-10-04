// Demo modules are currently commented out
// use weavegraph::{run_demo1, run_demo2, run_demo3, run_demo4};
use miette::Result;
use tracing_error::ErrorLayer;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

fn init_tracing() {
    let fmt_layer = fmt::layer()
        .with_target(false)
        .with_file(false)
        .with_line_number(false)
        // Log when spans are created/closed so we see instrumented async boundaries
        .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE);

    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("error,weavegraph=error"))
        .unwrap();

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .with(ErrorLayer::default())
        .init();
}

fn init_miette() {
    // Pretty panic reports
    miette::set_panic_hook();
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    init_miette();

    // Very small CLI: cargo run -- [demo]
    // Examples: cargo run -- demo1 | demo2 | demo3
    // Note: Demo functions are currently commented out
    let which = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "demo4".to_string());
    match which.as_str() {
        "demo1" | "demo2" | "demo3" | "demo4" => {
            println!("Demo functionality is currently disabled.");
            println!("The demo functions are commented out during refactoring.");
        }
        _ => println!(
            "Invalid demo option. Available options when enabled: demo1, demo2, demo3, demo4"
        ),
    }
    Ok(())
}
