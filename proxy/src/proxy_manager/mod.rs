// 2022-2024 (c) Copyright Contributors to the GOSH DAO. All rights reserved.

use std::collections::HashSet;
use std::process::exit;
use std::thread;
use std::time::Duration;

use anyhow::Context;
use clap::Parser;

use crate::config::store::ConfigStore;

pub mod blockchain;
pub mod cli;

const CONFIG_CHECK_INTERVAL: Duration = Duration::from_secs(5);

pub fn run() -> Result<(), std::io::Error> {
    eprintln!("Starting proxy manager...");
    dotenvy::dotenv().ok(); // ignore all errors and load what we can
    crate::tracing::init_tracing();
    tracing::info!("Starting...");

    if let Err(err) = thread::Builder::new().name("tokio_main".into()).spawn(tokio_main)?.join() {
        tracing::error!("tokio main thread panicked: {:#?}", err);
        exit(1);
    }

    exit(0);
}

#[tokio::main]
async fn tokio_main() -> anyhow::Result<()> {
    let args = cli::CliArgs::parse();

    // NOTE: doesn't catch panic!
    if let Err(err) = proxy_manager(args).await {
        tracing::error!("{err}");
        exit(1);
    }

    exit(0);
}

pub async fn proxy_manager(args: cli::CliArgs) -> anyhow::Result<()> {
    tracing::info!("Starting proxy manager...");

    loop {
        let config = ConfigStore::try_load(&args.proxy_config)?;

        let proxy_set: HashSet<String> = blockchain::get_proxy_list().await?.into_iter().collect();
        tracing::debug!("proxy set: {proxy_set:?}");

        let all_outers: HashSet<String> = config
            .config
            .connections
            .iter()
            .filter_map(|(_, connection)| connection.outer.as_ref().map(|outer| outer.url.clone()))
            .collect();
        tracing::debug!("all outers: {all_outers:?}");

        let enabled_outers: HashSet<String> = config
            .config
            .connections
            .iter()
            .filter_map(|(_, connection)| match &connection.outer {
                Some(outer) if outer.enabled => Some(outer.url.clone()),
                _ => None,
            })
            .collect();
        tracing::debug!("enabled outers: {enabled_outers:?}");

        if proxy_set != enabled_outers {
            let not_in_all: HashSet<&str> =
                proxy_set.difference(&all_outers).map(|s| s.as_str()).collect();

            if !not_in_all.is_empty() {
                tracing::error!(
                    "proxy list has proxies we don't have credentials for: {not_in_all:?}"
                );
                exit(1);
            }

            let mut config = config;

            for connection in config.config.connections.values_mut() {
                if let Some(outer) = &connection.outer {
                    if proxy_set.contains(&outer.url) {
                        connection.outer.as_mut().unwrap().enabled = true;
                    }
                }
            }

            config.save(&args.proxy_config)?;
            tracing::info!("Updated config");

            reload_proxy(&args.command).await?;
            tracing::info!("Reloaded proxy");
        }
        std::thread::sleep(CONFIG_CHECK_INTERVAL);
    }
}

async fn reload_proxy(settings: &cli::Command) -> anyhow::Result<()> {
    match settings {
        cli::Command::Docker { socket, container } => {
            let docker = docker_api::Docker::new(socket).context("Failed to create docker api")?;
            let container = docker.containers().get(container);
            tracing::info!("Sending SIGHUP to container: {:?}", container);
            container.kill(Some("SIGHUP")).await?;
        }
        cli::Command::PidPath { pid_path: _ } => {
            unimplemented!("pid path")
        }
        cli::Command::Pid { pid } => {
            std::process::Command::new("kill")
                .arg("-HUP")
                .arg(pid.to_string())
                .output()
                .context("Failed to kill proxy")?;
        }
    }
    Ok(())
}
