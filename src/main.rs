use cc_uplink::mcp;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("serve") | None => mcp::serve().await,
        // TEMPORARY: cli::run lands in Task 15. Any non-"serve" argument is
        // rejected with a usage message rather than crashing or silently
        // doing nothing.
        Some(_other) => {
            eprintln!("usage: cc-uplink [serve|doctor|send|invoke|log]");
            std::process::exit(2);
        }
    }
}
