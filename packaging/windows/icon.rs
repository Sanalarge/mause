#[path = "../../src/logo.rs"]
mod logo;

fn main() {
    let sizes = [16u32, 32, 48, 256];
    let mut images = Vec::new();
    for size in sizes {
        let pixels = logo::rgba(size as usize);
        let mask = size.div_ceil(32) * 4 * size;
        let mut data = Vec::new();
        for n in [40u32, size, size * 2] {
            data.extend(n.to_le_bytes());
        }
        data.extend(1u16.to_le_bytes());
        data.extend(32u16.to_le_bytes());
        data.extend(0u32.to_le_bytes());
        data.extend((size * size * 4 + mask).to_le_bytes());
        data.extend([0u8; 16]);
        for row in (0..size as usize).rev() {
            for col in 0..size as usize {
                let i = (row * size as usize + col) * 4;
                data.extend([pixels[i + 2], pixels[i + 1], pixels[i], pixels[i + 3]]);
            }
        }
        data.resize(data.len() + mask as usize, 0);
        images.push(data);
    }
    let mut out = vec![0, 0, 1, 0, sizes.len() as u8, 0];
    let mut offset = 6 + sizes.len() * 16;
    for (size, data) in sizes.iter().zip(&images) {
        out.extend([*size as u8, *size as u8, 0, 0]);
        out.extend(1u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        out.extend((data.len() as u32).to_le_bytes());
        out.extend((offset as u32).to_le_bytes());
        offset += data.len();
    }
    for data in images {
        out.extend(data);
    }
    std::fs::write(std::env::args_os().nth(1).expect("output ICO path"), out).unwrap();
}
