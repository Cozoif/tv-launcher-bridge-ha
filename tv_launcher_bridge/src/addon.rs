mod adb;
mod bridge;
mod config;
mod ha;
mod mdns;
mod server;
mod spake2;
mod tls;
fn main() {
    let cfg = config::Config::load();
    assert!(cfg.api_token.len() >= 24, "Set an API token of at least 24 characters");
    let bridge = bridge::Bridge::new(cfg);
    let api = server::ApiServer::new();
    api.start(&bridge);
    assert!(bridge.shared.lock().unwrap().api_running, "API failed to start");
    // Each status request has bounded connection attempts. No endless retry loop.
    loop { std::thread::park(); }
}
