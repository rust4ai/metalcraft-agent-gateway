//! Thin binary wrapper. All logic lives in the library (`lib.rs`) so the
//! gateway can be embedded by the `starkbot-metal` umbrella crate.

#[tokio::main]
async fn main() {
    metalcraft_agent_gateway::run().await;
}
