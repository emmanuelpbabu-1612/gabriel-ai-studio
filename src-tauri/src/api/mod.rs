pub mod routes;
pub mod router;

use std::net::SocketAddr;

use crate::core::engine::EngineState;

pub async fn serve(engine: EngineState) -> std::io::Result<()> {
    let cfg = engine.config().clone();
    let addr = SocketAddr::new(
        cfg.host
            .parse()
            .unwrap_or_else(|_| std::net::IpAddr::from([127, 0, 0, 1])),
        cfg.port,
    );

    let app = router::build_router(engine);
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            // Loud, diagnosable failure: the generic io error ("Only one usage
            // of each socket address... (os error 10048)") hides the cause.
            // This almost always means a stale gabriel.exe from a previous
            // `pnpm tauri dev` run is still holding the port.
            tracing::error!(
                "Port {} already in use — another Gabriel instance may be running \
                 (REST API bind on {addr} failed). Close the existing gabriel.exe \
                 (or run `pnpm run kill:stale`) and retry.",
                cfg.port,
            );
            return Err(e);
        }
        Err(e) => return Err(e),
    };

    tracing::info!("OpenAI-compatible API listening on http://{addr}/v1");
    axum::serve(listener, app).await
}
