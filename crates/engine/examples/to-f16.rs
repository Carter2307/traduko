//! Rewrites the weights of a Marian model at half the size.
//!
//!     cargo run --release -p coco-engine --example to-f16 -- IN OUT
//!
//! `IN` is a `model.safetensors` in 32-bit floats as the Hub has it for the
//! small opus-mt models; `OUT` holds the same tensors in 16-bit floats, the
//! form the engine computes in. The work is `coco_engine::to_f16`, which the
//! app uses too when it downloads the small models itself.

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [input, output] = arguments.as_slice() else {
        eprintln!("usage: to-f16 IN.safetensors OUT.safetensors");
        return ExitCode::from(2);
    };
    let (input, output) = (Path::new(input), Path::new(output));
    match coco_engine::to_f16(input, output) {
        Ok(tensors) => {
            let megabytes = |path: &Path| std::fs::metadata(path).map_or(0.0, |file| file.len() as f64 / 1e6);
            println!(
                "{} ({:.1} MB) -> {} ({:.1} MB, {tensors} tensors)",
                input.display(),
                megabytes(input),
                output.display(),
                megabytes(output)
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("to-f16: {error}");
            ExitCode::FAILURE
        }
    }
}
