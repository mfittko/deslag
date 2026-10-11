//! `deslag-proxy --upstream <url> [--listen <addr>]`

use std::{net::SocketAddr, process::ExitCode};

use deslag_proxy::{Upstream, serve};
use tokio::net::TcpListener;

const USAGE: &str = "usage: deslag-proxy --upstream <url> [--listen <addr>]";

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("deslag-proxy: {e}");
            ExitCode::from(2)
        }
    }
}

async fn run() -> Result<(), String> {
    let mut listen = "127.0.0.1:8789".to_string();
    let mut upstream = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value\n{USAGE}"));
        match arg.as_str() {
            "--listen" => listen = value()?,
            "--upstream" => upstream = Some(value()?),
            _ => return Err(format!("unknown argument: {arg}\n{USAGE}")),
        }
    }
    let upstream = Upstream::parse(&upstream.ok_or(USAGE)?)?;
    let addr: SocketAddr = listen
        .parse()
        .map_err(|e| format!("invalid --listen {listen}: {e}"))?;
    // PLAN §15: desktop defaults bind loopback only; network exposure needs authentication first.
    if !addr.ip().is_loopback() {
        return Err(format!("--listen must be a loopback address, got {addr}"));
    }
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| format!("cannot listen on {addr}: {e}"))?;
    eprintln!("deslag-proxy listening on {addr}, upstream {upstream}");
    serve(listener, upstream).await;
    Ok(())
}
