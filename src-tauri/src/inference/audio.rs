use crate::error::{GabrielError, Result};

pub const SAMPLE_RATE_HZ: u32 = 22_050;

pub struct WavBuilder {
    sample_rate: u32,
    samples: Vec<i16>,
}

impl WavBuilder {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            samples: Vec::new(),
        }
    }

    pub fn push_sample(&mut self, s: i16) {
        self.samples.push(s);
    }

    pub fn finish(self) -> Vec<u8> {
        self.encode()
    }

    fn encode(&self) -> Vec<u8> {
        let data_len = (self.samples.len() * 2) as u32;
        let mut out = Vec::with_capacity(44 + data_len as usize);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&self.sample_rate.to_le_bytes());
        out.extend_from_slice(&(self.sample_rate * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for s in &self.samples {
            out.extend_from_slice(&s.to_le_bytes());
        }
        out
    }
}

pub fn validate_dimensions(width: u32, height: u32) -> Result<(u32, u32)> {
    const MAX_DIM: u32 = 4096;
    if width == 0 || height == 0 || width > MAX_DIM || height > MAX_DIM {
        return Err(GabrielError::InvalidRequest(format!(
            "image dimensions {width}x{height} outside 1x1..{MAX_DIM}x{MAX_DIM}"
        )));
    }
    Ok((width, height))
}
