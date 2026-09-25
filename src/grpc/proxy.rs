use crate::config::ProxyConfig;
use crate::grpc::policy::check_rule;
use crate::kube::auth::KubeAuthClient;
use crate::utils::metrics::{REQUEST_DURATION, REQUEST_TOTAL};
use crate::utils::proxy::{evaluate_request, get_header, inject_headers, parse_bearer_token};
use async_trait::async_trait;
use pingora::http::ResponseHeader;
use pingora::prelude::*;
use pingora::proxy::{ProxyHttp, Session};
use std::sync::Arc;
use std::time::Instant;
use tracing::info;
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
        trailers.insert(
            "grpc-status",
            status_code
                .to_string()
                .parse()
                .map_err(|e| Error::because(ErrorType::InternalError, "invalid grpc-status header", e))?,
        );
        trailers.insert(
            "grpc-message",
            message
                .parse()
                .map_err(|e| Error::because(ErrorType::InternalError, "invalid grpc-message header", e))?,
        );
        session.downstream_session.write_response_trailers(trailers).await?;
        Ok(true)
    }
}

pub struct RequestContext {
    start: Instant,
}

#[async_trait]
impl ProxyHttp for GrpcProxy {
    type CTX = RequestContext;

    fn new_ctx(&self) -> Self::CTX {
        RequestContext { start: Instant::now() }
    }

    async fn request_filter(&self, session: &mut Session, _ctx: &mut Self::CTX) -> Result<bool> {
        let path = session.req_header().uri.path();
        let (service, action) = parse_grpc_path(path);
        let bearer_token = parse_bearer_token(session);

        let outcome = evaluate_request(
            bearer_token,
            &self.config.grpc.rules,
            &self.config.grpc.no_auth_rules,
            |matches| check_rule(matches, service, action, |h| get_header(session, h)),
            &self.client,
        )
        .await;

        match outcome {
            Ok(allowed) => {
                if let Some(auth_info) = &allowed.auth_info {
                    inject_headers(session, &self.config, auth_info)
                        .map_err(|e| Error::because(ErrorType::InternalError, "inject headers", e))?;
                }
                info!("rule matched: {:?}", allowed.rule_name);
                Ok(false)
            },
            Err(msg) => self.error_response(16, &msg, session).await,
        }
    }

    async fn upstream_peer(&self, _session: &mut Session, _ctx: &mut Self::CTX) -> Result<Box<HttpPeer>> {
        Ok(Box::new(self.peer.clone()))
    }

    async fn logging(&self, session: &mut Session, _e: Option<&Error>, ctx: &mut Self::CTX) {
        let status = session
            .response_written()
            .and_then(|r| r.status.as_str().parse::<u16>().ok())
            .unwrap_or(0)
            .to_string();
        let duration = ctx.start.elapsed().as_secs_f64();
        REQUEST_TOTAL.with_label_values(&["grpc", &status]).inc();
        REQUEST_DURATION.with_label_values(&["grpc", &status]).observe(duration);
    }
}

fn parse_grpc_path(path: &str) -> (&str, &str) {
    let trimmed = path.strip_prefix('/').unwrap_or(path);
    trimmed.rsplit_once('/').unwrap_or((trimmed, ""))
}
