use anyhow::Context as _;
use harness_server::{ServerConfig, start};

fn main() -> anyhow::Result<()> {
    if std::env::var_os("HARNESS_SERVER_URL").is_some() {
        return harness_ui::run();
    }

    let config = ServerConfig::embedded_from_environment()?;
    let server = start(config).context("failed to start the embedded Harness server")?;
    let endpoint = harness_ui::Endpoint::loopback(server.address().port());
    harness_ui::run_with_endpoint(endpoint)?;
    drop(server);
    Ok(())
}
