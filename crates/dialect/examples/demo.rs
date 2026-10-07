//! cargo run --release --example demo -- us|uk[-oxford] [off|safe|extended] "text"
use dialect::{convert_detailed, EnglishVariant, Options, Vocabulary};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: demo us|uk|uk-oxford [off|safe|extended] \"text\"");
        std::process::exit(2);
    }
    let (to, oxford) = match args[0].as_str() {
        "us" => (EnglishVariant::American, false),
        "uk" => (EnglishVariant::British, false),
        "uk-oxford" => (EnglishVariant::British, true),
        other => panic!("unknown variant {other}"),
    };
    let (vocabulary, text) = match args[1].as_str() {
        "off" => (Vocabulary::Off, args[2..].join(" ")),
        "safe" => (Vocabulary::Safe, args[2..].join(" ")),
        "extended" => (Vocabulary::Extended, args[2..].join(" ")),
        _ => (Vocabulary::Safe, args[1..].join(" ")),
    };
    let opts = Options { vocabulary, oxford_ize: oxford, ..Options::default() };
    let out = convert_detailed(&text, to, &opts);
    println!("{}", out.text);
    for c in &out.changes {
        eprintln!("  [{:?}] {:?} -> {:?} @{}..{}", c.kind, c.from, c.to, c.start, c.end);
    }
}
