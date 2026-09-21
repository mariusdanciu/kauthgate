use crate::config::ProxyConfig;
use crate::grpc::policy::check_rule;
use crate::kube::auth::KubeAuthClient;
use crate::utils::proxy::compile_resource_attributes;
use crate::utils::proxy::get_header;
use crate::utils::proxy::parse_bearer_token;
use async_trait::async_trait;
use pingora::http::ResponseHeader;
use pingora::prelude::*;
use pingora::proxy::{ProxyHttp, Session};
use std::sync::Arc;
use tracing::{error, info};

pub struct GrpcProxy {
    client: KubeAuthClient,
    config: Arc<ProxyConfig>,
    peer: HttpPeer,
}

impl GrpcProxy {
    pub fn new(config: Arc<ProxyConfig>, client: KubeAuthClient) -> Self {
        let mut peer = HttpPeer::new(
            (config.grpc.upstream.host.clone(), config.grpc.upstream.port),
            false,
            String::new(),
        );
        peer.options.set_http_version(2, 2);

        Self { config, client, peer }
    }

    async fn error_response(&self, status_code: u8, message: &str, session: &mut Session) -> Result<bool> {
        let mut resp = ResponseHeader::build(200, Some(2))?;
        resp.insert_header("content-type", "application/grpc")?;
        session.write_response_header(Box::new(resp), false).await?;

        let mut trailers = pingora::http::HMap::new();
        trailers.insert("grpc-status", status_code.to_string().parse().unwrap());
        trailers.insert("grpc-message", message.parse().unwrap());
        session.downstream_session.write_response_trailers(trailers).await?;
        Ok(true)
    }
}

#[async_trait]
impl ProxyHttp for GrpcProxy {
    type CTX = ();

    fn new_ctx(&self) -> Self::CTX {}

    async fn request_filter(&self, session: &mut Session, _ctx: &mut Self::CTX) -> Result<bool> {
        info!("request_filter");
        let path = session.req_header().uri.path();

        let bearer_token = parse_bearer_token(session);

        let (service, action) = parse_grpc_path(path);

        let auth_info = self.client.authenticate(bearer_token).await;

        match auth_info {
            Ok(auth_info) => {
                for policy in &self.config.grpc.rules {
                    if let Some(vars) = check_rule(policy, service, action, |header| get_header(session, header)) {
                        let resource_attributes = compile_resource_attributes(&policy.sar_resource_attributes, &vars);

                        let resp = self.client.authorize(&auth_info, &resource_attributes).await;

                        if let Err(e) = resp {
                            error!("authorization failed: {:?}", e);
                            return self.error_response(16, &e.to_string(), session).await;
                        }
                        return Ok(false); // Successfully authorized. Continue to upstream.
                    } else {
                        info!("policy does not match: {:?}", policy.name);
                    }
                }

                return self.error_response(16, "no policy matched", session).await;
            },
            Err(e) => {
                error!("authentication failed: {:?}", e);
                return self.error_response(16, &e.to_string(), session).await;
            },
        }
    }

    async fn upstream_peer(&self, _session: &mut Session, _ctx: &mut Self::CTX) -> Result<Box<HttpPeer>> {
        Ok(Box::new(self.peer.clone()))
    }
}

fn parse_grpc_path(path: &str) -> (&str, &str) {
    let trimmed = path.strip_prefix('/').unwrap_or(path);
    trimmed.rsplit_once('/').unwrap_or((trimmed, ""))
}
