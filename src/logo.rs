fn d_round_rect(p: (f32, f32), c: (f32, f32), half: (f32, f32), r: f32) -> f32 {
    let q = (
        (p.0 - c.0).abs() - half.0 + r,
        (p.1 - c.1).abs() - half.1 + r,
    );
    let outside = (q.0.max(0.0).powi(2) + q.1.max(0.0).powi(2)).sqrt();
    outside + q.0.max(q.1).min(0.0) - r
}

fn d_seg(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (pa, ba) = ((p.0 - a.0, p.1 - a.1), (b.0 - a.0, b.1 - a.1));
    let t = ((pa.0 * ba.0 + pa.1 * ba.1) / (ba.0 * ba.0 + ba.1 * ba.1)).clamp(0.0, 1.0);
    ((pa.0 - ba.0 * t).powi(2) + (pa.1 - ba.1 * t).powi(2)).sqrt()
}

const WHITE: [f32; 3] = [1.0, 1.0, 1.0];

const MOUSE: (f32, f32) = (0.5, 0.65);
const MOUSE_HALF: (f32, f32) = (0.165, 0.235);
const SPLIT: (f32, f32) = (0.5, 0.31);
const SCREENS: [(f32, f32); 2] = [(0.265, 0.165), (0.735, 0.165)];
const SCREEN_HALF: (f32, f32) = (0.1, 0.072);
const SCREEN_R: f32 = 0.03;
const LINE: f32 = 0.021;
const STAND: f32 = 0.05;

fn sample(u: f32, v: f32) -> [f32; 4] {
    let p = (u, v);
    if d_round_rect(p, (0.5, 0.5), (0.5, 0.5), 0.23) > 0.0 {
        return [0.0; 4];
    }
    let t = ((u + v) * 0.5).clamp(0.0, 1.0);
    let mut c = [1.0, 0.64 - 0.22 * t, 0.10 - 0.10 * t];

    let top = (MOUSE.0, MOUSE.1 - MOUSE_HALF.1);
    let mut cable = d_seg(p, top, SPLIT);
    let mut frames = f32::MAX;
    for s in SCREENS {
        let bottom = (s.0, s.1 + SCREEN_HALF.1);
        let foot = (s.0, bottom.1 + STAND);
        cable = cable.min(d_seg(p, bottom, foot)).min(d_seg(p, foot, SPLIT));
        frames = frames.min(d_round_rect(p, s, SCREEN_HALF, SCREEN_R).abs());
    }
    if cable < LINE || frames < LINE {
        c = WHITE;
    }
    if d_round_rect(p, MOUSE, MOUSE_HALF, MOUSE_HALF.0) < 0.0 {
        let wheel = d_seg(p, (MOUSE.0, 0.5), (MOUSE.0, 0.565)) < 0.022;
        if !wheel {
            c = WHITE;
        }
    }
    [c[0], c[1], c[2], 1.0]
}

pub fn rgba(n: usize) -> Vec<u8> {
    const SS: usize = 4;
    let mut out = vec![0u8; n * n * 4];
    let s = n as f32;
    for y in 0..n {
        for x in 0..n {
            let mut acc = [0f32; 4];
            for sy in 0..SS {
                for sx in 0..SS {
                    let u = (x as f32 + (sx as f32 + 0.5) / SS as f32) / s;
                    let v = (y as f32 + (sy as f32 + 0.5) / SS as f32) / s;
                    let c = sample(u, v);
                    for k in 0..3 {
                        acc[k] += c[k] * c[3];
                    }
                    acc[3] += c[3];
                }
            }
            let i = (y * n + x) * 4;
            if acc[3] > 0.0 {
                for k in 0..3 {
                    out[i + k] = (acc[k] / acc[3] * 255.0) as u8;
                }
                out[i + 3] = (acc[3] / (SS * SS) as f32 * 255.0) as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore]
    fn logo_onizleme() {
        let Ok(path) = std::env::var("MAUSE_LOGO") else {
            return;
        };
        let (w, h, split) = (1400usize, 640usize, 760usize);
        let mut img = vec![0u8; w * h * 3];
        for y in 0..h {
            for x in 0..w {
                let bg: [u8; 3] = if x < split {
                    [0x14, 0x14, 0x16]
                } else {
                    [0xf1, 0xf2, 0xee]
                };
                img[(y * w + x) * 3..][..3].copy_from_slice(&bg);
            }
        }
        let mut put = |n: usize, ox: usize, oy: usize| {
            let px = rgba(n);
            for y in 0..n {
                for x in 0..n {
                    let s = &px[(y * n + x) * 4..][..4];
                    let a = s[3] as f32 / 255.0;
                    let d = &mut img[((oy + y) * w + ox + x) * 3..][..3];
                    for k in 0..3 {
                        d[k] = (s[k] as f32 * a + d[k] as f32 * (1.0 - a)) as u8;
                    }
                }
            }
        };
        for (n, x) in [(512, 40), (64, 580), (32, 660), (16, 700)] {
            put(n, x, 64);
        }
        for (n, x) in [(256, 800), (128, 1080), (64, 1220), (32, 1300), (16, 1350)] {
            put(n, x, 64);
        }
        let mut jpeg = Vec::new();
        jpeg_encoder::Encoder::new(&mut jpeg, 95)
            .encode(&img, w as u16, h as u16, jpeg_encoder::ColorType::Rgb)
            .unwrap();
        std::fs::write(path, jpeg).unwrap();
    }
}
