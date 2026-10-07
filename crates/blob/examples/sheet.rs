//! Draws Coco's poses and a filmstrip of its motion into an SVG file, to
//! look at the animation without opening a window.
//!
//!     cargo run -p coco-blob --example sheet -- out.svg

use coco_blob::{CANVAS, Frame, Mascot, Mood};

const CELL: f32 = 120.0;
const COLUMNS: usize = 12;

fn outline(points: &[[f32; 2]], ox: f32, oy: f32) -> String {
    let k = CELL / (2.0 * CANVAS);
    let mut d = String::new();
    for (i, p) in points.iter().enumerate() {
        let (x, y) = (ox + (p[0] + CANVAS) * k, oy + (p[1] + CANVAS) * k);
        d += &format!("{}{x:.2} {y:.2}", if i == 0 { "M" } else { "L" });
    }
    d + "Z"
}

fn cell(frame: &Frame, index: usize, label: &str, out: &mut String) {
    let (ox, oy) = ((index % COLUMNS) as f32 * CELL, (index / COLUMNS) as f32 * (CELL + 14.0));
    let [r, g, b] = frame.color.map(|c| (c * 255.0).round() as u8);
    *out += &format!("<path d=\"{}\" fill=\"rgb({r},{g},{b})\"/>", outline(&frame.body, ox, oy));
    for eye in &frame.eyes {
        *out += &format!("<path d=\"{}\" fill=\"#fff\"/>", outline(eye, ox, oy));
    }
    *out += &format!(
        "<text x=\"{}\" y=\"{}\" font-family=\"Menlo\" font-size=\"9\" fill=\"#888\" text-anchor=\"middle\">{label}</text>",
        ox + CELL / 2.0,
        oy + CELL + 8.0
    );
}

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "sheet.svg".into());
    let mut cells: Vec<(Frame, String)> = Vec::new();

    for mood in [Mood::Idle, Mood::Thinking, Mood::Happy, Mood::Sorry, Mood::Waking] {
        let mut m = Mascot::new(mood);
        m.step(0.0);
        cells.push((m.frame(), format!("{mood:?}")));
    }
    while cells.len() % COLUMNS != 0 {
        cells.push((Frame::default(), String::new()));
    }

    // A scripted scene, one cell every 4 frames at 60 fps.
    let script: [(f32, &str, fn(&mut Mascot)); 9] = [
        (0.20, "pointer", |m| m.set_pointer(Some([0.9, -0.5]))),
        (0.70, "press", |m| m.press()),
        (0.95, "release", |m| { m.release(); m.set_pointer(None) }),
        (1.60, "thinking", |m| m.set_mood(Mood::Thinking)),
        (3.00, "happy", |m| m.set_mood(Mood::Happy)),
        (5.20, "drag", |m| m.drag([7.0, 1.0])),
        (5.70, "drop", |m| m.drop_it()),
        (6.40, "sorry", |m| m.set_mood(Mood::Sorry)),
        (7.40, "idle", |m| m.set_mood(Mood::Idle)),
    ];
    let mut m = Mascot::default();
    let mut next = 0;
    let mut label = "idle";
    for i in 0..(60 * 8 + 30) {
        let t = i as f32 / 60.0;
        while next < script.len() && script[next].0 <= t {
            script[next].2(&mut m);
            label = script[next].1;
            next += 1;
        }
        m.step(1.0 / 60.0);
        if i % 4 == 0 {
            cells.push((m.frame(), format!("{t:.2}s {label}")));
        }
    }

    let rows = cells.len().div_ceil(COLUMNS);
    let (w, h) = (COLUMNS as f32 * CELL, rows as f32 * (CELL + 14.0));
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w} {h}\" width=\"{w}\" height=\"{h}\"><rect width=\"{w}\" height=\"{h}\" fill=\"#f2f2f2\"/>"
    );
    for (i, (frame, label)) in cells.iter().enumerate() {
        cell(frame, i, label, &mut svg);
    }
    svg += "</svg>";
    std::fs::write(&path, svg).expect("write the sheet");
    println!("{} cells -> {path}", cells.len());
}
