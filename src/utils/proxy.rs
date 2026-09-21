use pingora::proxy::Session;

pub(crate) fn get_header(session: &Session, header: &str) -> Option<String> {
    session
        .req_header()
        .headers
        .get(header)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string())
}
