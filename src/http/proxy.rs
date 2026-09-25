use crate::config::ProxyConfig;
use crate::http::policy::check_rule;
use crate::kube::auth::KubeAuthClient;
use crate::utils::metrics::{REQUEST_DURATION, REQUEST_TOTAL};
use crate::utils::proxy::AuthorizationInfo;
use crate::utils::proxy::get_header;
use crate::utils::proxy::parse_bearer_token;
use crate::utils::proxy::run_authz;
use async_trait::async_trait;
use pingora::http::ResponseHeader;
use pingora::prelude::*;
use pingora::proxy::{ProxyHttp, Session};
use std::cell::OnceCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tracing::{error, info};
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

        let query_map: OnceCell<HashMap<&str, &str>> = OnceCell::new();
        match parse_bearer_token(session) {
            Some(bearer_token) => {
                let auth_info = self.client.authenticate(bearer_token).await;

                match auth_info {
                    Ok(auth_info) => {
                        for rule in &self.config.http.rules {
                            if let Some(vars) = check_rule(
                                &rule.request,
                                path,
                                method,
                                |header| get_header(session, header),
                                |param| {
                                    query_map
                                        .get_or_init(|| parse_query(session.req_header().uri.query()))
                                        .get(param)
                                        .map(|v| v.to_string())
                                },
                            ) {
                                if let Err(e) = run_authz(
                                    session,
                                    &AuthorizationInfo {
                                        sar_resource_attributes: &rule.sar,
                                        vars: &vars,
                                        client: &self.client,
                                        auth_info: &auth_info,
                                        config: &self.config,
                                    },
                                )
                                .await
                                {
                                    return self.error_response(403, &e.to_string(), session).await;
                                }
                                return Ok(false);
                            } else {
                                info!("rule does not match: {:?}", rule.name);
                            }
                        }
                    },
                    Err(e) => {
                        error!("authentication failed: {:?}", e);
                        return self.error_response(401, &e.to_string(), session).await;
                    },
                }
            },
            None => {
                for rule in &self.config.http.no_auth_rules {
                    if let Some(_) = check_rule(
                        &rule.request,
                        path,
                        method,
                        |header| get_header(session, header),
                        |param| {
                            query_map
                                .get_or_init(|| parse_query(session.req_header().uri.query()))
                                .get(param)
                                .map(|v| v.to_string())
                        },
                    ) {
                        return Ok(false);
                    }
                }
            },
        }

        self.error_response(16, "no rule matched", session).await
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
