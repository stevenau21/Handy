//! Diagnostic binary: try to load the wake-word model and print the full
//! error. Used to debug model-loading failures.

use std::path::Path;

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "../resources/models/hey_livekit.onnx".to_string());
    let p = Path::new(&path);
    println!("Loading model at: {}", p.display());
    println!("Exists: {}", p.exists());
    if !p.exists() {
        eprintln!("File not found!");
        std::process::exit(1);
    }
    println!("File size: {} bytes", std::fs::metadata(p).map(|m| m.len()).unwrap_or(0));

    println!("Calling WakeWordModel::new...");
    let result = livekit_wakeword::WakeWordModel::new(&[p], 16_000);
    match result {
        Ok(mut m) => {
            println!("✓ Model loaded successfully");
            // Try to feed 1 second of silence at 16 kHz to make sure inference works.
            let silence: Vec<i16> = vec![0i16; 16_000];
            let scores = m.predict(&silence);
            match scores {
                Ok(preds) => {
                    println!("✓ Prediction OK. Scores: {preds:?}");
                }
                Err(e) => {
                    eprintln!("✗ Prediction failed: {e}");
                }
            }
        }
        Err(e) => {
            eprintln!("✗ Model load failed: {e}");
            eprintln!("Full error chain:");
            let mut src: &dyn std::error::Error = &e;
            let mut depth = 0;
            while let Some(s) = src.source() {
                depth += 1;
                eprintln!("  {depth}. {s}");
                src = s;
            }
        }
    }
}
