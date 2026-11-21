pub const LIST: u8 = 0;
pub const CAMERA_START: u8 = 1;
pub const CAMERA_STOP: u8 = 2;
pub const TORCH: u8 = 3;
pub const VIBRATE: u8 = 4;
pub const STATUS: u8 = 5;
pub const METRICS_SUBSCRIBE: u8 = 6;
pub const METRICS_SAMPLE: u8 = 7;
pub const CAMERA_PORT: u16 = 24807;

#[derive(Clone, Debug, PartialEq)]
pub struct Cap {
    pub kind: String,
    pub id: String,
    pub label: String,
    pub rotation: u16,
}

pub fn parse_caps(text: &str) -> Vec<Cap> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(4, '|');
            let kind = parts.next().filter(|kind| !kind.is_empty())?;
            let id = parts.next()?;
            let label = parts.next()?;
            let rotation = parts
                .next()
                .and_then(|r| r.trim().parse().ok())
                .unwrap_or(0);
            Some(Cap {
                kind: kind.to_owned(),
                id: id.to_owned(),
                label: label.to_owned(),
                rotation: rotation % 360,
            })
        })
        .collect()
}

pub fn status_value<'a>(status: &'a str, key: &str) -> Option<&'a str> {
    status
        .split(';')
        .find_map(|pair| pair.split_once('=').filter(|(name, _)| *name == key))
        .map(|(_, value)| value)
}

pub fn status_line(status: &str) -> String {
    let mut parts = Vec::new();
    if let Some(level) = status_value(status, "battery") {
        let charging = status_value(status, "charging") == Some("1");
        parts.push(format!(
            "Pil %{level}{}",
            if charging { " (şarjda)" } else { "" }
        ));
    }
    if let (Some(lat), Some(lon)) = (status_value(status, "lat"), status_value(status, "lon")) {
        parts.push(format!("Konum {lat}, {lon}"));
    }
    parts.join(" · ")
}

pub fn rotate_rgb(
    width: usize,
    height: usize,
    rgb: &[u8],
    quarter_turns: u16,
) -> (usize, usize, Vec<u8>) {
    let turns = quarter_turns % 4;
    if turns == 0 || rgb.len() != width * height * 3 {
        return (width, height, rgb.to_vec());
    }
    let (out_width, out_height) = if turns == 2 {
        (width, height)
    } else {
        (height, width)
    };
    let mut out = vec![0u8; rgb.len()];
    for y in 0..height {
        for x in 0..width {
            let (nx, ny) = match turns {
                1 => (height - 1 - y, x),
                2 => (width - 1 - x, height - 1 - y),
                _ => (y, width - 1 - x),
            };
            let from = (y * width + x) * 3;
            let to = (ny * out_width + nx) * 3;
            out[to..to + 3].copy_from_slice(&rgb[from..from + 3]);
        }
    }
    (out_width, out_height, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yetenek_listesi_okunur() {
        let caps = parse_caps(
            "camera|0|Arka kamera|90\ncamera|1|Ön kamera|270\ntorch||El feneri|0\nbozuk\n",
        );
        assert_eq!(caps.len(), 3);
        assert_eq!(caps[0].id, "0");
        assert_eq!(caps[1].rotation, 270);
        assert_eq!(caps[2].kind, "torch");
        assert!(parse_caps("").is_empty());
    }

    #[test]
    fn durum_satiri_pil_ve_konumu_gosterir() {
        let status = "battery=85;charging=1;lat=41.01;lon=28.97";
        assert_eq!(status_value(status, "battery"), Some("85"));
        assert_eq!(status_line(status), "Pil %85 (şarjda) · Konum 41.01, 28.97");
        assert_eq!(status_line("battery=20;charging=0"), "Pil %20");
        assert_eq!(status_line(""), "");
    }

    #[test]
    fn goruntu_dondurulur() {
        let pixels: Vec<u8> = vec![1, 1, 1, 2, 2, 2, 3, 3, 3, 4, 4, 4, 5, 5, 5, 6, 6, 6];
        let (w, h, turned) = rotate_rgb(3, 2, &pixels, 1);
        assert_eq!((w, h), (2, 3));
        assert_eq!(turned[0], 4);
        assert_eq!(turned[(2 * w + 1) * 3], 3);
        let (w, h, half) = rotate_rgb(3, 2, &pixels, 2);
        assert_eq!((w, h, half[0]), (3, 2, 6));
        let (_, _, back) = rotate_rgb(3, 2, &pixels, 4);
        assert_eq!(back, pixels);
    }
}
