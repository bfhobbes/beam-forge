//! AutoCAD Color Index (ACI) to RGB.

/// `#rrggbb` for ACI `index` (1-255). Index 7 is "white on black, black on
/// white"; on a laser bed it is black.
pub(crate) fn aci_hex(index: u8) -> String {
    let [r, g, b] = aci_rgb(index);
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn aci_rgb(index: u8) -> [u8; 3] {
    match index {
        1 => [255, 0, 0],
        2 => [255, 255, 0],
        3 => [0, 255, 0],
        4 => [0, 255, 255],
        5 => [0, 0, 255],
        6 => [255, 0, 255],
        0 | 7 => [0, 0, 0],
        8 => [128, 128, 128],
        9 => [192, 192, 192],
        250..=255 => {
            const GREYS: [u8; 6] = [51, 91, 132, 173, 214, 255];
            let v = GREYS[usize::from(index - 250)];
            [v, v, v]
        }
        _ => {
            // 10-249: 24 hues 15° apart, each in five shades, full and
            // half saturation.
            let hue = f64::from(index / 10 - 1) * 15.0;
            let shade = index % 10;
            const VALUES: [f64; 5] = [1.0, 0.8, 0.6, 0.5, 0.3];
            let value = VALUES[usize::from(shade / 2)];
            let saturation = if shade.is_multiple_of(2) { 1.0 } else { 0.5 };
            hsv(hue, saturation, value)
        }
    }
}

fn hsv(hue: f64, saturation: f64, value: f64) -> [u8; 3] {
    let c = value * saturation;
    let h = hue / 60.0;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = value - c;
    let byte = |v: f64| ((v + m) * 255.0).round() as u8;
    [byte(r), byte(g), byte(b)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_colours() {
        assert_eq!(aci_hex(1), "#ff0000");
        assert_eq!(aci_hex(5), "#0000ff");
        assert_eq!(aci_hex(7), "#000000");
        assert_eq!(aci_hex(10), "#ff0000");
        assert_eq!(aci_hex(11), "#ff8080");
        assert_eq!(aci_hex(30), "#ff8000");
        assert_eq!(aci_hex(150), "#0080ff");
        assert_eq!(aci_hex(250), "#333333");
    }
}
