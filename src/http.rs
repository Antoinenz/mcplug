//! One HTTP client for everything: rustls, gzip, timeouts, and the unique User-Agent
//! Modrinth's terms require (`Antoinenz/mcplug/<version> (contact)`).

pub fn client(contact: Option<&str>) -> reqwest::Client {
    let ua = match contact {
        Some(c) if !c.is_empty() => format!("Antoinenz/mcplug/{} ({c})", env!("CARGO_PKG_VERSION")),
        _ => format!("Antoinenz/mcplug/{}", env!("CARGO_PKG_VERSION")),
    };
    reqwest::Client::builder()
        .user_agent(ua)
        .timeout(std::time::Duration::from_secs(60))
        .connect_timeout(std::time::Duration::from_secs(10))
        .gzip(true)
        .build()
        .expect("reqwest client")
}
