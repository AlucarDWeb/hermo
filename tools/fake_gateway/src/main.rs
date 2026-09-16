//! hermes-fake-gateway: a local stand-in gateway server for exercising the
//! core client against recorded fixtures instead of a live server.

use std::path::PathBuf;

use hermes_fake_gateway::server;

/// The recorded fixtures live in the core crate and are read, never written.
const CORE_FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../hermes_core/tests/fixtures");

fn print_usage() {
    println!("hermes-fake-gateway [flags]");
    println!();
    println!("  --port <port>          listen port (default: 9123)");
    println!("  --user <name>          login username (default: hermo)");
    println!("  --password <password>  login password (default: hermo)");
    println!(
        "  --fixture <path>       events fixture (default: ../../hermes_core/tests/fixtures/events.jsonl)"
    );
    println!(
        "  --synthetic <path>     synthetic fixture (default: ../../hermes_core/tests/fixtures/events_synthetic.jsonl)"
    );
    println!("  --drop-after <n>       drop the websocket connection after n frames");
    println!("  --name <name>          pairing display name (default: fake)");
    println!("  --help                 print this message");
}

#[tokio::main]
async fn main() {
    let mut port: u16 = 9123;
    let mut user = "hermo".to_string();
    let mut password = "hermo".to_string();
    let mut fixture = PathBuf::from(CORE_FIXTURES).join("events.jsonl");
    let mut synthetic = PathBuf::from(CORE_FIXTURES).join("events_synthetic.jsonl");
    let mut drop_after: Option<usize> = None;
    let mut name = "fake".to_string();

    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut take = |name: &str| -> String {
            match it.next() {
                Some(v) => v,
                None => {
                    eprintln!("hermes-fake-gateway: {name} requires a value");
                    std::process::exit(2);
                }
            }
        };
        match flag.as_str() {
            "--port" => {
                let v = take("--port");
                port = v.parse().unwrap_or_else(|_| {
                    eprintln!("hermes-fake-gateway: --port must be a valid port number");
                    std::process::exit(2);
                });
            }
            "--user" => user = take("--user"),
            "--password" => password = take("--password"),
            "--fixture" => fixture = PathBuf::from(take("--fixture")),
            "--synthetic" => synthetic = PathBuf::from(take("--synthetic")),
            "--drop-after" => {
                let v = take("--drop-after");
                let n: usize = v.parse().unwrap_or_else(|_| {
                    eprintln!("hermes-fake-gateway: --drop-after must be a number");
                    std::process::exit(2);
                });
                drop_after = Some(n);
            }
            "--name" => name = take("--name"),
            "--help" => {
                print_usage();
                std::process::exit(0);
            }
            other => {
                eprintln!("hermes-fake-gateway: unknown flag {other}");
                std::process::exit(2);
            }
        }
    }

    let config = server::Config {
        port,
        user: user.clone(),
        password,
        fixture,
        synthetic: Some(synthetic),
        drop_after,
    };

    let running = match server::start(config).await {
        Ok(running) => running,
        Err(e) => {
            eprintln!("hermes-fake-gateway: failed to start: {e}");
            std::process::exit(1);
        }
    };

    println!("{}", running.base_url);
    println!("{}", server::pairing_payload(&running.base_url, &user, &name));

    tokio::signal::ctrl_c()
        .await
        .expect("ctrl-c signal handler installs cleanly");
}
