use anyhow::Result;
use clap::Parser;
use grpc::proxy::GrpcProxy;
use http::proxy::HttpProxy;
use kube::auth::KubeAuthClient;
use logging::init_tracing;
use pingora::prelude::*;
use pingora::proxy::http_proxy_service;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

mod config;
mod grpc;
mod http;
mod kube;
mod logging;
mod utils;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct CommandLineArgs {
    /// Enable JSON logs
    #[arg(short, long, default_value = "false")]
    json_logs: bool,

    /// Config file for this server
    #[arg(short, long, default_value = "config/config.yaml")]
    config: String,

    /// Optional additional config file (e.g. a mounted Secret) merged on top
    /// of `config`; missing values here fall back to `config`.
    #[arg(long, default_value = "/secrets/secret-config.yaml")]
    secret_config: String,
}

fn main() -> Result<()> {
    init_tracing(false);

    let args = CommandLineArgs::parse();
    let config = config::load_config(args.config, args.secret_config)?;

    info!("config: {:?}", config);

    let kube_auth = KubeAuthClient::new(
        Duration::from_secs(config.auth.cache_ttl_secs),
        config.auth.token_review_audiences.clone(),
    );

    let config = Arc::new(config);

    let mut server = Server::new(None).unwrap();
    server.bootstrap();

    let grpc_proxy = GrpcProxy::new(config.clone(), kube_auth.clone());
    let mut grpc_service = http_proxy_service(&server.configuration, grpc_proxy);
    let mut h2c_options = pingora::apps::HttpServerOptions::default();
    h2c_options.h2c = true;
    grpc_service.app_logic_mut().unwrap().server_options = Some(h2c_options);
    grpc_service.add_tcp("0.0.0.0:6188");
    server.add_service(grpc_service);

    let http_proxy = HttpProxy::new(config.clone(), kube_auth);
    let mut http_service = http_proxy_service(&server.configuration, http_proxy);
    http_service.add_tcp("0.0.0.0:8080");
    server.add_service(http_service);

    server.run_forever();
}
