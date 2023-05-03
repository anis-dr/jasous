use std::sync::Arc;

use crate::{
    helpers::{check_address_block, host_addr},
    tunnels::{tunnel, tunnel_with_throttling},
    HttpClient,
};
use hyper::{Body, Method, Request, Response};
use tracing::{error, info, instrument};

#[instrument]
pub async fn proxy_with_optional_throttling(
    client: &Arc<HttpClient>,
    req: Request<Body>,
    buffer_size: Option<usize>,
    delay_millis: Option<u64>,
) -> Result<Response<Body>, hyper::Error> {
    info!("Incoming request: {:?}", req);
    info!("Incoming request headers: {:?}", req.headers());

    if let Some(addr) = host_addr(req.uri()) {
        info!("Checking address block: {}", addr);
        if check_address_block(&addr) {
            // Return a forbidden response if the address is blocked
            let mut resp = Response::new(Body::empty());
            *resp.status_mut() = http::StatusCode::FORBIDDEN;
            return Ok(resp);
        }
    }

    if Method::CONNECT == req.method() {
        if let Some(addr) = host_addr(req.uri()) {
            info!("Request target address: {}", addr);
            tokio::task::spawn(async move {
                match hyper::upgrade::on(req).await {
                    Ok(upgraded) => {
                        let tunnel_result = match (buffer_size, delay_millis) {
                            (Some(buffer_size), Some(delay_millis)) => {
                                tunnel_with_throttling(upgraded, addr, buffer_size, delay_millis)
                                    .await
                            }
                            _ => tunnel(upgraded, addr).await,
                        };

                        if let Err(e) = tunnel_result {
                            error!("Server IO error: {}", e);
                        };
                    }
                    Err(e) => error!("Upgrade error: {}", e),
                }
            });

            Ok(Response::new(Body::empty()))
        } else {
            error!("CONNECT host is not socket addr: {:?}", req.uri());
            let mut resp = Response::new(Body::from("CONNECT must be to a socket address"));
            *resp.status_mut() = http::StatusCode::BAD_REQUEST;

            Ok(resp)
        }
    } else {
        client.request(req).await
    }
}
