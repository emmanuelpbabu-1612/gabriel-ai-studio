use axum::extract::State;
use axum::Json;

use crate::core::engine::EngineState;
use crate::error::{ApiError, GabrielError};
use crate::inference::audio::validate_dimensions;
use crate::types::openai::{
    ImageData, ImageGenerationRequest, ImageGenerationResponse,
};

pub async fn images_generations(
    State(engine): State<EngineState>,
    Json(req): Json<ImageGenerationRequest>,
) -> Result<Json<ImageGenerationResponse>, ApiError> {
    if req.prompt.trim().is_empty() {
        return Err(ApiError(GabrielError::InvalidRequest(
            "prompt must not be empty".into(),
        )));
    }
    if req.prompt.chars().count() > 4_096 {
        return Err(ApiError(GabrielError::InvalidRequest(
            "prompt exceeds the 4096 character limit".into(),
        )));
    }

    let (width, height) = parse_size(req.size.as_deref())?;
    let n = req.n.clamp(1, 4);
    let model_id = req.model.unwrap_or_else(|| "gabriel-diffusion".into());

    let mut data = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let rx = engine
            .submit_image(&model_id, req.prompt.clone(), width, height)
            .await
            .map_err(ApiError)?;

        let bytes = rx
            .await
            .map_err(|_| ApiError(GabrielError::Internal("image job dropped".into())))?
            .map_err(ApiError)?;

        data.push(ImageData {
            b64_json: base64_encode(&bytes),
        });
    }

    Ok(Json(ImageGenerationResponse {
        created: crate::types::ipc::unix_now(),
        data,
    }))
}

fn parse_size(size: Option<&str>) -> crate::error::Result<(u32, u32)> {
    let Some(raw) = size else {
        return validate_dimensions(512, 512);
    };
    let (w, h) = raw
        .split_once('x')
        .ok_or_else(|| GabrielError::InvalidRequest(format!("invalid size '{raw}', expected WxH")))?;
    let width = w
        .trim()
        .parse::<u32>()
        .map_err(|_| GabrielError::InvalidRequest(format!("invalid width '{w}'")))?;
    let height = h
        .trim()
        .parse::<u32>()
        .map_err(|_| GabrielError::InvalidRequest(format!("invalid height '{h}'")))?;
    validate_dimensions(width, height)
}

const BASE64_TABLE: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(BASE64_TABLE[(n >> 18) as usize & 63] as char);
        out.push(BASE64_TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            BASE64_TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            BASE64_TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}
