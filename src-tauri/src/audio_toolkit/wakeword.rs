//! Wake-word detection wrapper around the vendored `livekit-wakeword` crate.
//!
//! This module listens to i16 PCM audio chunks and emits a confidence score
//! for each pre-trained wake-word classifier (e.g., "hey_livekit.onnx").
//!
//! It is intentionally decoupled from the audio pipeline: the consumer
//! (e.g. `AudioRecordingManager`) decides what to do with the score.

use anyhow::{Context, Result};
use livekit_wakeword::WakeWordModel;
use log::{debug, info, warn};
use std::error::Error;
use std::path::Path;
use std::sync::Mutex;

/// Default confidence threshold above which we consider a wake word detected.
/// 0.5 is a balanced default (tuneable via settings).
pub const DEFAULT_THRESHOLD: f32 = 0.5;

/// Number of samples required for a valid wake-word prediction (~2 seconds at
/// 16 kHz, or the equivalent after internal resampling for other rates).
/// The `WakeWordModel::predict` API will return zero scores for shorter
/// chunks, so we buffer until we have enough.
const PREDICT_CHUNK_SAMPLES: usize = 32_000; // 2.0 s @ 16 kHz

/// State for the wake-word detector.
pub struct WakeWordDetector {
    /// Loaded ONNX model. We wrap in `Mutex` because `predict` takes `&mut self`.
    model: Mutex<Option<WakeWordModel>>,
    /// Rolling buffer of i16 samples that we feed to the model.
    buffer: Mutex<Vec<i16>>,
    /// Names of the loaded classifiers (e.g. "hey_livekit").
    classifier_names: Vec<String>,
}

impl WakeWordDetector {
    /// Create a new detector, loading the ONNX classifier from `model_path`.
    ///
    /// `sample_rate` should match the rate the audio is provided at in
    /// `feed_samples`. The vendored crate will resample internally to 16 kHz
    /// if a different rate is supplied.
    pub fn new(model_path: &Path, sample_rate: u32) -> Result<Self> {
        if !model_path.exists() {
            anyhow::bail!(
                "Wake word model not found at {}",
                model_path.display()
            );
        }

        // Construct the model with the given classifier file.
        // The vendored crate's `WakeWordModel::new` takes a slice of paths
        // and a sample rate. The model is loaded from disk here.
        let mut model = WakeWordModel::new(&[model_path], sample_rate).map_err(|e| {
            let mut chain = format!("WakeWordModel::new failed: {e}");
            let mut src: &dyn std::error::Error = &e;
            while let Some(cause) = src.source() {
                chain.push_str(&format!(" -> {cause}"));
                src = cause;
            }
            warn!("{chain}");
            anyhow::anyhow!("{chain}")
        })?;

        // Use the file stem as the classifier name for friendlier logging.
        let stem = model_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("wakeword")
            .to_string();
        // We don't strictly need to re-load — `new` already loaded it — but
        // re-issuing `load_model` here would be a no-op. We just record the
        // name for diagnostics.
        let classifier_names = vec![stem.clone()];
        info!(
            "Loaded wake word classifier '{}' (sample rate {} Hz, threshold {})",
            stem, sample_rate, DEFAULT_THRESHOLD
        );

        // Touch the model so the compiler doesn't drop it.
        let _ = &mut model;

        Ok(Self {
            model: Mutex::new(Some(model)),
            buffer: Mutex::new(Vec::with_capacity(PREDICT_CHUNK_SAMPLES * 2)),
            classifier_names,
        })
    }

    /// Feed a chunk of mono i16 PCM samples to the detector.
    ///
    /// Returns the highest confidence score across all loaded classifiers
    /// if a prediction was run on this chunk, or `None` if the internal
    /// buffer has not yet accumulated enough samples for a prediction.
    pub fn feed_samples(&self, samples: &[i16]) -> Result<Option<f32>> {
        // Append to rolling buffer.
        {
            let mut buf = self.buffer.lock().expect("wakeword buffer poisoned");
            buf.extend_from_slice(samples);
            if buf.len() < PREDICT_CHUNK_SAMPLES {
                return Ok(None);
            }

            // We have enough samples; drain exactly one prediction window.
            // (We keep any extra in the buffer for the next call so we
            // don't drop audio.)
            let to_predict: Vec<i16> = buf.drain(..PREDICT_CHUNK_SAMPLES).collect();
            // Lock is released at end of this block.
            let mut model_guard = self.model.lock().expect("wakeword model poisoned");
            let model = match model_guard.as_mut() {
                Some(m) => m,
                None => {
                    warn!("WakeWordModel not initialised; skipping prediction");
                    return Ok(None);
                }
            };

            match model.predict(&to_predict) {
                Ok(predictions) => {
                    // `predictions` is a HashMap<String, f32>.
                    // The `livekit-wakeword` API returns a score per classifier.
                    // We return the maximum so the caller can apply a threshold.
                    let max_score = predictions
                        .values()
                        .copied()
                        .fold(f32::NEG_INFINITY, f32::max);
                    if max_score.is_finite() {
                        debug!(
                            "Wake-word prediction scores: {:?} (max={:.3})",
                            predictions, max_score
                        );
                    } else {
                        debug!("Wake-word model returned no usable scores");
                    }
                    Ok(Some(max_score))
                }
                Err(e) => {
                    warn!("Wake-word prediction failed: {e}");
                    Ok(None)
                }
            }
        }
    }

    /// Names of the loaded classifiers (for diagnostics / settings UI).
    pub fn classifier_names(&self) -> &[String] {
        &self.classifier_names
    }
}
