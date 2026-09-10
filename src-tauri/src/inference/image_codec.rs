pub fn encode_bmp_rgba(width: u32, height: u32, seed: u64) -> Vec<u8> {
    let row_padding = (4 - (width * 3) % 4) % 4;
    let stride = width * 3 + row_padding;
    let pixel_data_size = stride * height;
    let file_size = 54 + pixel_data_size;

    let mut buf = Vec::with_capacity(file_size as usize);
    buf.extend_from_slice(b"BM");
    buf.extend_from_slice(&file_size.to_le_bytes());
    buf.extend_from_slice(&[0; 4]);
    buf.extend_from_slice(&54u32.to_le_bytes());

    buf.extend_from_slice(&40u32.to_le_bytes());
    buf.extend_from_slice(&(width as i32).to_le_bytes());
    buf.extend_from_slice(&(height as i32).to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&24u16.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&pixel_data_size.to_le_bytes());
    buf.extend_from_slice(&2835u32.to_le_bytes());
    buf.extend_from_slice(&2835u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());

    let hue_a = (seed % 360) as f32;
    let hue_b = ((seed >> 16) % 360) as f32;

    for y in 0..height {
        for x in 0..width {
            let t_x = x as f32 / width.max(1) as f32;
            let t_y = y as f32 / height.max(1) as f32;
            let t = (t_x + t_y) / 2.0;
            let hue = lerp_hue(hue_a, hue_b, t);
            let (r, g, b) = hsv_to_rgb(hue, 0.65 - 0.25 * t_y, 0.55 + 0.35 * t);
            buf.push(blue_channel(r, g, b));
            buf.push(g);
            buf.push(r);
        }
        for _ in 0..row_padding {
            buf.extend_from_slice(&[0u8]);
        }
    }

    buf
}

fn blue_channel(_r: u8, _g: u8, b: u8) -> u8 {
    b
}

fn lerp_hue(a: f32, b: f32, t: f32) -> f32 {
    let d = ((b - a + 180.0).rem_euclid(360.0)) - 180.0;
    (a + d * t).rem_euclid(360.0)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let hp = h / 60.0;
    let x = c * (1.0 - (hp.rem_euclid(2.0) - 1.0).abs());
    let (r1, g1, b1) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    (
        ((r1 + m) * 255.0) as u8,
        ((g1 + m) * 255.0) as u8,
        ((b1 + m) * 255.0) as u8,
    )
}

pub fn hash_string(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for byte in s.as_bytes() {
        h ^= *byte as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}
