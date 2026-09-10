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
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!("OpenAI-compatible API listening on http://{addr}/v1");
    axum::serve(listener, app).await
}
