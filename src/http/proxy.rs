use crate::config::ProxyConfig;
use crate::http::policy::check_mapping;
use crate::kube::auth::KubeAuthClient;
use crate::utils::proxy::compile_resource_attributes;
use crate::utils::proxy::get_header;
use async_trait::async_trait;
use pingora::http::ResponseHeader;
use pingora::prelude::*;
use pingora::proxy::{ProxyHttp, Session};
use std::cell::OnceCell;
use std::collections::HashMap;
use std::sync::Arc;
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

#[async_trait]
impl ProxyHttp for HttpProxy {
    type CTX = ();

    fn new_ctx(&self) -> Self::CTX {}

    async fn request_filter(&self, session: &mut Session, _ctx: &mut Self::CTX) -> Result<bool> {
        let path = session.req_header().uri.path();
        let method = session.req_header().method.as_str();

        let bearer_token = session
            .req_header()
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("");

        let auth_info = self.client.authenticate(bearer_token).await;

        match auth_info {
            Ok(auth_info) => {
                let query_map: OnceCell<HashMap<&str, &str>> = OnceCell::new();
                for policy in &self.config.http.mappings {
                    if let Some(vars) = check_mapping(
                        policy,
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
                        let resource_attributes = compile_resource_attributes(&policy.sar_resource_attributes, &vars);

                        let resp = self.client.authorize(&auth_info, &resource_attributes).await;

                        if let Err(e) = resp {
                            error!("authorization failed: {:?}", e);
                            return self.error_response(403, &e.to_string(), session).await;
                        }
                        return Ok(false);
                    } else {
                        info!("policy does not match: {:?}", policy.name);
                    }
                }

                return self.error_response(403, "no policy matched", session).await;
            },
            Err(e) => {
                error!("authentication failed: {:?}", e);
                return self.error_response(401, &e.to_string(), session).await;
            },
        }
    }

    async fn upstream_peer(&self, _session: &mut Session, _ctx: &mut Self::CTX) -> Result<Box<HttpPeer>> {
        Ok(Box::new(self.peer.clone()))
    }
}

fn parse_query(query: Option<&str>) -> HashMap<&str, &str> {
    query
        .map(|q| q.split('&').filter_map(|pair| pair.split_once('=')).collect())
        .unwrap_or_default()
}
