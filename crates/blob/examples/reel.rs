//! Plays a script of events on Traduko's mascot and writes every frame as JSON.
//! usage: cargo run -p traduko-blob --example reel -- <script> <out.json> <seconds> [fps]
use std::fmt::Write;
use traduko_blob::{Mascot, Mood};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let script = std::fs::read_to_string(&args[1]).unwrap();
    let seconds: f32 = args[3].parse().unwrap();
    let fps: f32 = args.get(4).map_or(60.0, |v| v.parse().unwrap());
    let mut events: Vec<(f32, Vec<String>)> = script
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let mut parts = l.split_whitespace().map(String::from);
            let t: f32 = parts.next().unwrap().parse().unwrap();
            (t, parts.collect())
        })
        .collect();
    events.sort_by(|a, b| a.0.total_cmp(&b.0));

    let mut mascot = Mascot::new(Mood::Idle);
    mascot.breathe = true;
    let mut drag: Option<[f32; 2]> = None;
    let mut next = 0;
    let mut out = String::from("[");
    let frames = (seconds * fps).round() as usize;
    for i in 0..frames {
        let t = i as f32 / fps;
        while next < events.len() && events[next].0 <= t {
            let e = &events[next].1;
            let f = |k: usize| e[k].parse::<f32>().unwrap();
            match e[0].as_str() {
                "mood" => mascot.set_mood(match e[1].as_str() {
                    "idle" => Mood::Idle,
                    "thinking" => Mood::Thinking,
                    "happy" => Mood::Happy,
                    "sorry" => Mood::Sorry,
                    "waking" => Mood::Waking,
                    other => panic!("mood {other}"),
                }),
                "enter" => mascot.enter(),
                "nudge" => mascot.nudge(),
                "press" => mascot.press(),
                "release" => mascot.release(),
                "drag" => drag = Some([f(1), f(2)]),
                "drop" => {
                    drag = None;
                    mascot.drop_it();
                }
                "pointer" => mascot.set_pointer(if e[1] == "none" { None } else { Some([f(1), f(2)]) }),
                "attention" => mascot.set_attention(if e[1] == "none" { None } else { Some([f(1), f(2)]) }),
                "color" => {
                    let hex = u32::from_str_radix(&e[1], 16).unwrap();
                    mascot.set_color([16, 8, 0].map(|s| ((hex >> s) & 0xff) as f32 / 255.0));
                }
                other => panic!("event {other}"),
            }
            next += 1;
        }
        if let Some(v) = drag {
            mascot.drag(v);
        }
        mascot.step(1.0 / fps);
        let frame = mascot.frame();
        let pts = |p: &Vec<[f32; 2]>| p.iter().map(|q| format!("{:.3},{:.3}", q[0], q[1])).collect::<Vec<_>>().join(",");
        let c = frame.color.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
        if i > 0 {
            out.push(',');
        }
        write!(out, "{{\"c\":\"#{:02x}{:02x}{:02x}\",\"b\":[{}],\"e\":[[{}],[{}]]}}", c[0], c[1], c[2], pts(&frame.body), pts(&frame.eyes[0]), pts(&frame.eyes[1])).unwrap();
    }
    out.push(']');
    std::fs::write(&args[2], out).unwrap();
}
