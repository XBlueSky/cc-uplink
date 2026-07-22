use cc_uplink::{cli, mcp};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("serve") | None => mcp::serve().await,
        Some(other) => cli::run(other, &args[1..]).await,
    }
}
