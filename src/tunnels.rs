use hyper::upgrade::Upgraded;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::sleep;

use tracing::{error, info, instrument, span, Level};

use std::time::Duration;

// Create a TCP connection to host:port, build a tunnel between the connection and
// the upgraded connection
#[instrument]
pub async fn tunnel(mut upgraded: Upgraded, addr: String) -> std::io::Result<()> {
    let tunnel_span = span!(Level::TRACE, "tunnel", addr = %addr);
    let _enter = tunnel_span.enter();

    info!(
        "Tunneling Attempting to connect to target host and port: {}",
        addr.clone()
    );
    // Connect to remote server
    let mut server = TcpStream::connect(addr).await?;

    // Proxying data
    let (from_client, from_server) =
        match tokio::io::copy_bidirectional(&mut upgraded, &mut server).await {
            Ok((rom_client, from_server)) => (rom_client, from_server),
            Err(e) => {
                error!("Error during bidirectional copy: {}", e);
                return Err(e);
            }
        };

    // Print message when done
    info!(
        "client wrote {} bytes and received {} bytes",
        from_client, from_server
    );

    Ok(())
}

#[instrument]
pub async fn tunnel_with_throttling(
    upgraded: Upgraded,
    addr: String,
    buffer_size: usize,
    delay_millis: u64,
) -> std::io::Result<()> {
    let tunnel_span = span!(Level::TRACE, "tunnel", addr = %addr);
    let _enter = tunnel_span.enter();

    info!(
        "Tunneling Attempting to connect to target host and port: {}",
        addr.clone()
    );
    // Connect to remote server
    let server = TcpStream::connect(addr).await?;

    // Split the connections into read and write halves
    let (mut upgraded_read, mut upgraded_write) = tokio::io::split(upgraded);
    let (mut server_read, mut server_write) = tokio::io::split(server);

    // Proxying data
    let client_to_server = async {
        let mut buffer = vec![0; buffer_size];
        loop {
            match upgraded_read.read(&mut buffer).await {
                Ok(n) if n == 0 => break,
                Ok(n) => {
                    sleep(Duration::from_millis(delay_millis)).await;
                    server_write.write_all(&buffer[..n]).await?;
                }
                Err(e) => {
                    error!("Error reading from client: {}", e);
                    break;
                }
            }
        }
        Ok::<(), std::io::Error>(())
    };

    let server_to_client = async {
        let mut buffer = vec![0; buffer_size];
        loop {
            match server_read.read(&mut buffer).await {
                Ok(n) if n == 0 => break,
                Ok(n) => {
                    sleep(Duration::from_millis(delay_millis)).await;
                    upgraded_write.write_all(&buffer[..n]).await?;
                }
                Err(e) => {
                    error!("Error reading from server: {}", e);
                    break;
                }
            }
        }
        Ok::<(), std::io::Error>(())
    };

    let proxy_result = tokio::try_join!(client_to_server, server_to_client);

    match proxy_result {
        Ok((_, _)) => info!("Tunneling completed"),
        Err(e) => error!("Tunneling error: {}", e),
    }

    Ok(())
}
