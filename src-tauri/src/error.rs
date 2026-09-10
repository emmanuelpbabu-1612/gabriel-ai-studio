use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::ser::SerializeStruct;
use serde_json::json;

#[derive(thiserror::Error, Debug)]
pub enum GabrielError {
    #[error("model not loaded: {0}")]
    ModelNotLoaded(String),
    #[error("model already loaded on device: {0}")]
    AlreadyLoaded(String),
    #[error("unknown model type: {0}")]
    UnknownModelType(String),
    #[error(
        "insufficient VRAM to admit request: required {required_bytes} bytes, available {available_bytes} bytes"
    )]
    VramExhausted {
        required_bytes: u64,
        available_bytes: u64,
    },
    #[error("model slot pool exhausted: at most {limit} models may be registered concurrently")]
    SlotPoolExhausted { limit: usize },
    #[error("failed to fetch '{filename}' from hub repo '{repo}'")]
    DownloadFailed { repo: String, filename: String },
    #[error("failed to load weights for model '{model_id}': {detail}")]
    WeightLoadFailed { model_id: String, detail: String },
    #[error("GPU memory query failed")]
    GpuQueryFailed,
    #[error("inference kernel failure: {0}")]
    Kernel(String),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("inference backend failure: {0}")]
    Backend(String),
    #[error("job queue rejected job {job_id}: {reason}")]
    QueueRejected { job_id: String, reason: String },
    #[error("internal error: {0}")]
    Internal(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, GabrielError>;

#[cfg(any(feature = "candle-cuda", feature = "tts-parler"))]
impl From<candle_core::Error> for GabrielError {
    fn from(e: candle_core::Error) -> Self {
        Self::Kernel(format!("{e}"))
    }
}

impl GabrielError {
    fn http_status(&self) -> StatusCode {
        match self {
            Self::ModelNotLoaded(_) => StatusCode::NOT_FOUND,
            Self::AlreadyLoaded(_) => StatusCode::CONFLICT,
            Self::UnknownModelType(_) | Self::InvalidRequest(_) => StatusCode::BAD_REQUEST,
            Self::VramExhausted { .. } | Self::SlotPoolExhausted { .. } => {
                StatusCode::INSUFFICIENT_STORAGE
            }
            Self::DownloadFailed { .. } => StatusCode::SERVICE_UNAVAILABLE,
            Self::WeightLoadFailed { .. } | Self::GpuQueryFailed | Self::Kernel(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
            Self::QueueRejected { .. } | Self::Backend(_) | Self::Internal(_) | Self::Io(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }

    pub fn code_slug(&self) -> &'static str {
        match self {
            Self::ModelNotLoaded(_) => "model_not_loaded",
            Self::AlreadyLoaded(_) => "model_already_loaded",
            Self::UnknownModelType(_) => "unknown_model_type",
            Self::VramExhausted { .. } => "vram_exhausted",
            Self::SlotPoolExhausted { .. } => "slot_pool_exhausted",
            Self::DownloadFailed { .. } => "download_failed",
            Self::WeightLoadFailed { .. } => "weight_load_failed",
            Self::GpuQueryFailed => "gpu_query_failed",
            Self::Kernel(_) => "kernel_failure",
            Self::InvalidRequest(_) => "invalid_request",
            Self::QueueRejected { .. } => "queue_rejected",
            Self::Backend(_) => "backend_failure",
            Self::Internal(_) => "internal_error",
            Self::Io(_) => "io_error",
        }
    }
}

impl serde::Serialize for GabrielError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut s = serializer.serialize_struct("GabrielError", 3)?;
        s.serialize_field("code", self.code_slug())?;
        s.serialize_field("message", &self.to_string())?;
        s.serialize_field("ok", &false)?;
        s.end()
    }
}

pub struct ApiError(pub GabrielError);

impl From<GabrielError> for ApiError {
    fn from(e: GabrielError) -> Self {
        Self(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.0.http_status();
        let body = json!({
            "error": {
                "message": self.0.to_string(),
                "type": self.0.code_slug(),
                "code": self.0.code_slug(),
            }
        });
        (status, axum::Json(body)).into_response()
    }
}
