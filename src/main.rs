use std::convert::Infallible;
use std::env;
use std::net::SocketAddr;
use std::sync::Arc;

use hyper::service::{make_service_fn, service_fn};
use hyper::{Client, Server};
use hyper_tls::HttpsConnector;

use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;

use dotenv::dotenv;

use crate::proxies::proxy_with_optional_throttling;

pub type HttpClient = Client<HttpsConnector<hyper::client::HttpConnector>>;

mod helpers;
mod proxies;
mod tunnels;

#[tokio::main]
async fn main() {
    dotenv().ok(); // Load the environment variables from the .env file

    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::TRACE) // Adjust the log level filter here
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("Failed to set subscriber");

    let addr = SocketAddr::from(([127, 0, 0, 1], 9000));
    let https_connector = HttpsConnector::new();
    let client = Arc::new(
        Client::builder()
            .http1_title_case_headers(true)
            .http1_preserve_header_case(true)
            .build(https_connector),
    );

    let throttling_is_enabled = env::var("ENABLE_THROTTLING")
        .map(|value| value.to_lowercase() == "true")
        .unwrap_or(false);

    let buffer_size = env::var("BUFFER_SIZE")
        .unwrap_or_else(|_| "4096".to_string())
        .parse::<usize>()
        .expect("BUFFER_SIZE must be a valid integer");

    let delay_millis = env::var("DELAY_MILLIS")
        .unwrap_or_else(|_| "200".to_string())
        .parse::<u64>()
        .expect("DELAY_MILLIS must be a valid integer");

    let make_service = make_service_fn(move |_| {
        let client = client.clone();
        async move {
            Ok::<_, Infallible>(service_fn(move |req| {
                let client = client.clone();
                async move {
                    if throttling_is_enabled {
                        proxy_with_optional_throttling(
                            &client,
                            req,
                            Some(buffer_size),
                            Some(delay_millis),
                        )
                        .await
                    } else {
                        proxy_with_optional_throttling(&client, req, None, None).await
                    }
                }
            }))
        }
    });

    let server = Server::bind(&addr)
        .http1_preserve_header_case(true)
        .http1_title_case_headers(true)
        .serve(make_service);

    info!("Proxy server started on http://{}", addr);

    if throttling_is_enabled {
        info!("Throttling enabled with BUFFER_SIZE: {}", buffer_size);
        info!("Throttling enabled with DELAY: {}ms", delay_millis);
    }

    if let Err(e) = server.await {
        error!("server error: {}", e);
    }
}
