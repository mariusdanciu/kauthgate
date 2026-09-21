use crate::config::ProxyConfig;
use crate::http::policy::check_mapping;
use crate::kube::auth::KubeAuthClient;
use crate::utils::proxy::compile_resource_attributes;
use crate::utils::proxy::get_header;
use async_trait::async_trait;
use pingora::http::ResponseHeader;
use pingora::prelude::*;
use pingora::proxy::{ProxyHttp, Session};
use std::sync::Arc;
use tracing::{error, info};

pub struct HttpProxy {
    client: KubeAuthClient,
    config: Arc<ProxyConfig>,
}

impl HttpProxy {
    pub fn new(config: Arc<ProxyConfig>, client: KubeAuthClient) -> Self {
        Self { config, client }
    }

    async fn error_response(
        &self,
        status_code: u16,
        message: &str,
        session: &mut Session,
    ) -> Result<bool> {
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
        let path = session.req_header().uri.path().to_string();
        let method = session.req_header().method.to_string().to_lowercase();

        let authorization = get_header(session, "authorization").unwrap_or("".to_string());

        let bearer_token = authorization.strip_prefix("Bearer ").unwrap_or("");

        let auth_info = self.client.authenticate(bearer_token).await;

        match auth_info {
            Ok(auth_info) => {
                for policy in &self.config.http.mappings {
                    if let Some(vars) = check_mapping(
                        policy,
                        &path,
                        &method,
                        |header| {
                            get_header(session, header)
                        },
                        |param| {
                            session.req_header().uri.query().and_then(|q| {
                                q.split('&')
                                    .filter_map(|pair| pair.split_once('='))
                                    .find(|(k, _)| *k == param)
                                    .map(|(_, v)| v.to_string())
                            })
                        },
                    ) {
                        let resource_attributes =
                            compile_resource_attributes(&policy.sar_resource_attributes, &vars);

                        let resp = self
                            .client
                            .authorize(&auth_info, &resource_attributes)
                            .await;

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
            }
            Err(e) => {
                error!("authentication failed: {:?}", e);
                return self.error_response(401, &e.to_string(), session).await;
            }
        }
    }

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        let peer = HttpPeer::new(
            (
                self.config.http.upstream.host.clone(),
                self.config.http.upstream.port,
            ),
            false,
            String::new(),
        );
        Ok(Box::new(peer))
    }
}
