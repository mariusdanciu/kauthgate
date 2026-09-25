use crate::config::ProxyConfig;
use crate::http::policy::check_rule;
use crate::kube::auth::KubeAuthClient;
use crate::utils::metrics::{REQUEST_DURATION, REQUEST_TOTAL};
use crate::utils::proxy::{evaluate_request, get_header, inject_headers, parse_bearer_token};
use async_trait::async_trait;
use pingora::http::ResponseHeader;
use pingora::prelude::*;
use pingora::proxy::{ProxyHttp, Session};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Instant;
use tracing::info;
pub struct HttpProxy {
    client: KubeAuthClient,
    config: Arc<ProxyConfig>,
    peer: HttpPeer,
}

impl HttpProxy {
    pub fn new(config: Arc<ProxyConfig>, client: KubeAuthClient) -> Self {
        let peer = HttpPeer::new(
            (config.http.upstream.host.clone(), config.http.upstream.port),
            false,
            String::new(),
        );
        Self { config, client, peer }
    }

    async fn error_response(&self, status_code: u16, message: &str, session: &mut Session) -> Result<bool> {
        let resp = ResponseHeader::build(status_code, Some(1))?;
        session.write_response_header(Box::new(resp), false).await?;
        let body: Vec<u8> = message.as_bytes().to_vec();
        session.write_response_body(Some(body.into()), true).await?;
        Ok(true)
    }
}

pub struct RequestContext {
    start: Instant,
}

#[async_trait]
impl ProxyHttp for HttpProxy {
    type CTX = RequestContext;

    fn new_ctx(&self) -> Self::CTX {
        RequestContext { start: Instant::now() }
    }

    async fn request_filter(&self, session: &mut Session, _ctx: &mut Self::CTX) -> Result<bool> {
        let path = session.req_header().uri.path();
        let method = session.req_header().method.as_str();
        let bearer_token = parse_bearer_token(session);
        let query_map: OnceLock<HashMap<&str, &str>> = OnceLock::new();

        let outcome = evaluate_request(
            bearer_token,
            &self.config.http.rules,
            &self.config.http.no_auth_rules,
            |matches| {
                check_rule(
                    matches,
                    path,
                    method,
                    |header| get_header(session, header),
                    |param| {
                        query_map
                            .get_or_init(|| parse_query(session.req_header().uri.query()))
                            .get(param)
                            .map(|v| v.to_string())
                    },
                )
            },
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
            Err(msg) => self.error_response(403, &msg, session).await,
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
        REQUEST_TOTAL.with_label_values(&["http", &status]).inc();
        REQUEST_DURATION.with_label_values(&["http", &status]).observe(duration);
    }
}

fn parse_query(query: Option<&str>) -> HashMap<&str, &str> {
    query
        .map(|q| q.split('&').filter_map(|pair| pair.split_once('=')).collect())
        .unwrap_or_default()
}
