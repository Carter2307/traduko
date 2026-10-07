//! Draws the app icon: Traduko on a light rounded square, 1024 by 1024.
//!
//!     cargo run --release -p traduko-blob --example icon -- assets/icon-1024.png

use traduko_blob::{Mascot, Mood};

const SIZE: usize = 1024;
/// Sub-samples per pixel side, for smooth edges.
const SUB: usize = 4;

struct Canvas {
    pixels: Vec<[f32; 4]>,
}

impl Canvas {
    /// Fills a closed outline with a colour, blending over what is there.
    fn fill(&mut self, outline: &[[f32; 2]], [r, g, b]: [f32; 3]) {
        let mut coverage = vec![0u8; SIZE * SIZE];
        let n = outline.len();
        for sub_row in 0..SIZE * SUB {
            let y = (sub_row as f32 + 0.5) / SUB as f32;
            let mut crossings: Vec<f32> = (0..n)
                .filter_map(|i| {
                    let (a, b) = (outline[i], outline[(i + 1) % n]);
                    ((a[1] <= y) != (b[1] <= y)).then(|| a[0] + (y - a[1]) / (b[1] - a[1]) * (b[0] - a[0]))
                })
                .collect();
            crossings.sort_by(f32::total_cmp);
            for span in crossings.chunks_exact(2) {
                let from = ((span[0] * SUB as f32 - 0.5).ceil().max(0.0)) as usize;
                let to = ((span[1] * SUB as f32 - 0.5).floor().min((SIZE * SUB - 1) as f32)) as usize;
                for sub_column in from..=to.max(from).min(SIZE * SUB - 1) {
                    if sub_column as f32 + 0.5 <= span[1] * SUB as f32 {
                        coverage[sub_row / SUB * SIZE + sub_column / SUB] += 1;
                    }
                }
            }
        }
        for (pixel, covered) in self.pixels.iter_mut().zip(coverage) {
            let a = covered as f32 / (SUB * SUB) as f32;
            *pixel = [
                r * a + pixel[0] * (1.0 - a),
                g * a + pixel[1] * (1.0 - a),
                b * a + pixel[2] * (1.0 - a),
                a + pixel[3] * (1.0 - a),
            ];
        }
    }
}

/// The rounded square macOS icons sit on: 824 wide in a 1024 canvas.
fn plate() -> Vec<[f32; 2]> {
    let (half, centre, power) = (412.0f32, 512.0f32, 5.0f32);
    (0..720)
        .map(|i| {
            let a = i as f32 / 720.0 * std::f32::consts::TAU;
            let (sin, cos) = a.sin_cos();
            let r = (cos.abs().powf(power) + sin.abs().powf(power)).powf(-1.0 / power);
            [centre + half * r * cos, centre + half * r * sin]
        })
        .collect()
}

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "icon-1024.png".into());
    let mut canvas = Canvas { pixels: vec![[0.0; 4]; SIZE * SIZE] };
    canvas.fill(&plate(), [0.957, 0.957, 0.961]);

    let frame = Mascot::new(Mood::Idle).frame();
    let place = |points: &[[f32; 2]]| -> Vec<[f32; 2]> { points.iter().map(|p| [512.0 + p[0] * 270.0, 500.0 + p[1] * 270.0]).collect() };
    canvas.fill(&place(&frame.body), frame.color);
    for eye in &frame.eyes {
        canvas.fill(&place(eye), [1.0, 1.0, 1.0]);
    }

    let bytes: Vec<u8> = canvas
        .pixels
        .iter()
        .flat_map(|[r, g, b, a]| {
            // PNG wants colour that is not multiplied by alpha.
            let un = |c: f32| if *a > 0.0 { (c / a).clamp(0.0, 1.0) } else { 0.0 };
            [un(*r), un(*g), un(*b), *a].map(|c| (c * 255.0).round() as u8)
        })
        .collect();
    let file = std::io::BufWriter::new(std::fs::File::create(&path).expect("create the icon file"));
    let mut encoder = png::Encoder::new(file, SIZE as u32, SIZE as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().and_then(|mut writer| writer.write_image_data(&bytes)).expect("write the icon");
    println!("{path}");
}
