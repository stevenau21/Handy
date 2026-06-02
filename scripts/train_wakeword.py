#!/usr/bin/env python3
"""
Train a custom wake-word classifier for Handy.

This script lets you train an ONNX classifier that detects your custom
wake phrase (e.g. "hey jarvis", "hey handy", "computer", etc.).

The classifier sits on top of the bundled mel spectrogram + embedding
models from the livekit-wakeword crate. You provide audio samples of:
  - POSITIVE: you saying your wake phrase
  - NEGATIVE: other speech, silence, background noise

The script extracts embedding sequences and trains a small neural network
classifier, then exports it as an ONNX model that Handy can load.

Usage:
  1. Install dependencies:
     pip install onnxruntime numpy torch soundfile

  2. Record positive samples (say your wake phrase 20-50 times):
     python train_wakeword.py record --label positive --count 30

  3. Record negative samples (other speech, silence, noise):
     python train_wakeword.py record --label negative --count 50

  4. Train the classifier:
     python train_wakeword.py train --wake-phrase "hey jarvis"

  5. Copy the model to Handy:
     cp output/hey_jarvis.onnx src-tauri/resources/models/

  6. Re-enable preload_wakeword() in src-tauri/src/managers/audio.rs
     and rebuild Handy.
"""

import argparse
import os
import sys
import time
import glob
import json
from pathlib import Path

import numpy as np

# ---------------------------------------------------------------------------
# Constants (must match the Rust crate)
# ---------------------------------------------------------------------------
SAMPLE_RATE = 16000
MEL_BINS = 32
EMBEDDING_WINDOW = 76   # mel frames per embedding
EMBEDDING_STRIDE = 8    # mel frames between embeddings
EMBEDDING_DIM = 96
MIN_EMBEDDINGS = 16     # classifier input length

# Paths to bundled ONNX models
SCRIPT_DIR = Path(__file__).resolve().parent
PROJECT_ROOT = SCRIPT_DIR.parent
MEL_ONNX = PROJECT_ROOT / "src-tauri" / "vendor" / "livekit-wakeword" / "onnx" / "melspectrogram.onnx"
EMB_ONNX = PROJECT_ROOT / "src-tauri" / "vendor" / "livekit-wakeword" / "onnx" / "embedding_model.onnx"

# Data and output directories
DATA_DIR = PROJECT_ROOT / "scripts" / "wakeword_data"
OUTPUT_DIR = PROJECT_ROOT / "scripts" / "wakeword_output"


def ensure_dirs():
    """Create data and output directories."""
    (DATA_DIR / "positive").mkdir(parents=True, exist_ok=True)
    (DATA_DIR / "negative").mkdir(parents=True, exist_ok=True)
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)


# ---------------------------------------------------------------------------
# Recording
# ---------------------------------------------------------------------------
def record_samples(label: str, count: int, duration: float = 2.0):
    """Record audio samples from the microphone and save as WAV files."""
    try:
        import sounddevice as sd
    except ImportError:
        print("ERROR: sounddevice not installed. Run: pip install sounddevice")
        sys.exit(1)

    ensure_dirs()
    out_dir = DATA_DIR / label
    existing = len(list(out_dir.glob("*.wav")))

    print(f"Recording {count} {label} samples ({duration}s each)")
    print(f"Save directory: {out_dir}")
    print(f"Existing samples: {existing}")
    print()

    if label == "positive":
        print(">>> Say your wake phrase clearly into the microphone")
    else:
        print(">>> Say random words, stay silent, or make background noise")

    print(">>> Press ENTER to start each recording, or Ctrl+C to stop early")
    print()

    for i in range(count):
        input(f"  [{i+1}/{count}] Press ENTER to record...")
        print(f"  Recording {duration}s...", end=" ", flush=True)

        audio = sd.rec(
            int(duration * SAMPLE_RATE),
            samplerate=SAMPLE_RATE,
            channels=1,
            dtype="float32",
        )
        sd.wait()
        print("done")

        filename = out_dir / f"{label}_{existing + i + 1:04d}.wav"

        try:
            import soundfile as sf
            sf.write(str(filename), audio, SAMPLE_RATE)
        except ImportError:
            # Fallback: save as raw numpy file
            filename = out_dir / f"{label}_{existing + i + 1:04d}.npy"
            np.save(str(filename), audio)

        print(f"  Saved: {filename.name}")

    print(f"\nDone! {count} {label} samples saved to {out_dir}")


# ---------------------------------------------------------------------------
# Feature extraction
# ---------------------------------------------------------------------------
class FeatureExtractor:
    """Extract embedding sequences from audio using bundled ONNX models."""

    def __init__(self):
        import onnxruntime as ort

        if not MEL_ONNX.exists():
            raise FileNotFoundError(f"Mel model not found: {MEL_ONNX}")
        if not EMB_ONNX.exists():
            raise FileNotFoundError(f"Embedding model not found: {EMB_ONNX}")

        providers = ["CPUExecutionProvider"]
        self.mel_session = ort.InferenceSession(str(MEL_ONNX), providers=providers)
        self.emb_session = ort.InferenceSession(str(EMB_ONNX), providers=providers)

        # Print model info
        print(f"Mel model input: {self.mel_session.get_inputs()[0].name} "
              f"shape={self.mel_session.get_inputs()[0].shape}")
        print(f"Mel model output: {self.mel_session.get_outputs()[0].name}")
        print(f"Embedding model input: {self.emb_session.get_inputs()[0].name} "
              f"shape={self.emb_session.get_inputs()[0].shape}")
        print(f"Embedding model output: {self.emb_session.get_outputs()[0].name}")
        print()

    def extract_embeddings(self, audio_f32: np.ndarray) -> np.ndarray:
        """
        Extract embedding sequence from audio.

        Args:
            audio_f32: float32 audio at 16kHz, shape (N,)

        Returns:
            Embedding sequence, shape (MIN_EMBEDDINGS, EMBEDDING_DIM)
            or None if audio is too short.
        """
        # Mel spectrogram
        mel_input_name = self.mel_session.get_inputs()[0].name
        mel_input = audio_f32.reshape(1, -1).astype(np.float32)
        mel_result = self.mel_session.run(None, {mel_input_name: mel_input})
        mel = mel_result[0]  # shape: (num_frames, MEL_BINS)

        num_frames = mel.shape[0]
        if num_frames < EMBEDDING_WINDOW:
            return None

        # Extract embeddings: sliding window of EMBEDDING_WINDOW, stride EMBEDDING_STRIDE
        embeddings = []
        start = 0
        while start + EMBEDDING_WINDOW <= num_frames:
            window = mel[start:start + EMBEDDING_WINDOW, :]  # (76, 32)
            emb_input_name = self.emb_session.get_inputs()[0].name
            emb_input = window.reshape(1, -1).astype(np.float32)  # (1, 76*32)
            emb_result = self.emb_session.run(None, {emb_input_name: emb_input})
            emb = emb_result[0]  # (1, 96)
            embeddings.append(emb[0])  # (96,)
            start += EMBEDDING_STRIDE

        if len(embeddings) < MIN_EMBEDDINGS:
            return None

        # Use last MIN_EMBEDDINGS embeddings
        embeddings = embeddings[-MIN_EMBEDDINGS:]
        return np.stack(embeddings, axis=0)  # (16, 96)

    def extract_from_file(self, filepath: str) -> np.ndarray:
        """Extract embeddings from a WAV or NPY file."""
        if filepath.endswith(".npy"):
            audio = np.load(filepath).flatten().astype(np.float32)
        else:
            try:
                import soundfile as sf
                audio, sr = sf.read(filepath)
                if sr != SAMPLE_RATE:
                    # Simple resampling (linear interpolation)
                    ratio = SAMPLE_RATE / sr
                    new_len = int(len(audio) * ratio)
                    indices = np.linspace(0, len(audio) - 1, new_len)
                    audio = np.interp(indices, np.arange(len(audio)), audio)
                audio = audio.flatten().astype(np.float32)
            except ImportError:
                print(f"ERROR: soundfile not installed. Run: pip install soundfile")
                return None

        return self.extract_embeddings(audio)


# ---------------------------------------------------------------------------
# Training
# ---------------------------------------------------------------------------
def collect_dataset(extractor: FeatureExtractor):
    """Collect all embedding sequences from recorded samples."""
    positive_data = []
    negative_data = []

    pos_dir = DATA_DIR / "positive"
    neg_dir = DATA_DIR / "negative"

    # Positive samples
    pos_files = sorted(list(pos_dir.glob("*.wav")) + list(pos_dir.glob("*.npy")))
    print(f"Processing {len(pos_files)} positive samples...")
    for f in pos_files:
        emb = extractor.extract_from_file(str(f))
        if emb is not None:
            positive_data.append(emb)
            print(f"  ✓ {f.name} → {emb.shape}")
        else:
            print(f"  ✗ {f.name} → too short, skipped")

    # Negative samples
    neg_files = sorted(list(neg_dir.glob("*.wav")) + list(neg_dir.glob("*.npy")))
    print(f"Processing {len(neg_files)} negative samples...")
    for f in neg_files:
        emb = extractor.extract_from_file(str(f))
        if emb is not None:
            negative_data.append(emb)
            print(f"  ✓ {f.name} → {emb.shape}")
        else:
            print(f"  ✗ {f.name} → too short, skipped")

    print(f"\nDataset: {len(positive_data)} positive, {len(negative_data)} negative")

    if len(positive_data) < 5:
        print("ERROR: Need at least 5 positive samples. Record more!")
        sys.exit(1)
    if len(negative_data) < 5:
        print("ERROR: Need at least 5 negative samples. Record more!")
        sys.exit(1)

    return positive_data, negative_data


def train_classifier(positive_data, negative_data, wake_phrase: str,
                     epochs: int = 100, lr: float = 0.001):
    """Train a small neural network classifier on embedding sequences."""
    try:
        import torch
        import torch.nn as nn
        import torch.optim as optim
    except ImportError:
        print("ERROR: PyTorch not installed. Run: pip install torch")
        sys.exit(1)

    # Build dataset
    X = np.stack(positive_data + negative_data, axis=0)  # (N, 16, 96)
    y = np.array(
        [1.0] * len(positive_data) + [0.0] * len(negative_data),
        dtype=np.float32,
    )

    # Shuffle
    indices = np.random.permutation(len(X))
    X = X[indices]
    y = y[indices]

    # Convert to tensors
    X_tensor = torch.FloatTensor(X)
    y_tensor = torch.FloatTensor(y).unsqueeze(1)

    # Define a small classifier
    # Input: (batch, 16, 96) → flatten → (batch, 1536)
    # Hidden layers → output: (batch, 1) sigmoid
    model = nn.Sequential(
        nn.Flatten(),                         # (B, 1536)
        nn.Linear(16 * 96, 128),              # (B, 128)
        nn.ReLU(),
        nn.Dropout(0.3),
        nn.Linear(128, 32),                   # (B, 32)
        nn.ReLU(),
        nn.Dropout(0.2),
        nn.Linear(32, 1),                     # (B, 1)
        nn.Sigmoid(),
    )

    # Training
    criterion = nn.BCELoss()
    optimizer = optim.Adam(model.parameters(), lr=lr)

    print(f"\nTraining classifier for '{wake_phrase}'...")
    print(f"  Samples: {len(X)} ({len(positive_data)} positive, {len(negative_data)} negative)")
    print(f"  Epochs: {epochs}, Learning rate: {lr}")
    print(f"  Model: Flatten(1536) → Dense(128) → ReLU → Dropout(0.3) → Dense(32) → ReLU → Dropout(0.2) → Dense(1) → Sigmoid")
    print()

    model.train()
    for epoch in range(epochs):
        optimizer.zero_grad()
        outputs = model(X_tensor)
        loss = criterion(outputs, y_tensor)
        loss.backward()
        optimizer.step()

        if (epoch + 1) % 10 == 0 or epoch == 0:
            preds = (outputs >= 0.5).float()
            acc = (preds == y_tensor).float().mean().item()
            print(f"  Epoch {epoch+1:3d}/{epochs} | Loss: {loss.item():.4f} | Acc: {acc:.2%}")

    # Evaluate
    model.eval()
    with torch.no_grad():
        outputs = model(X_tensor)
        preds = (outputs >= 0.5).float()
        acc = (preds == y_tensor).float().mean().item()
        pos_scores = outputs[:len(positive_data)].detach().numpy().flatten()
        neg_scores = outputs[len(positive_data):].detach().numpy().flatten()

    print(f"\nFinal accuracy: {acc:.2%}")
    print(f"Positive scores: mean={pos_scores.mean():.3f}, min={pos_scores.min():.3f}, max={pos_scores.max():.3f}")
    print(f"Negative scores: mean={neg_scores.mean():.3f}, min={neg_scores.min():.3f}, max={neg_scores.max():.3f}")

    return model


def export_onnx(model, wake_phrase: str):
    """Export the trained model as ONNX compatible with livekit-wakeword."""
    try:
        import torch
    except ImportError:
        print("ERROR: PyTorch not installed")
        sys.exit(1)

    ensure_dirs()

    # Sanitize filename
    filename = wake_phrase.replace(" ", "_").lower() + ".onnx"
    output_path = OUTPUT_DIR / filename

    # Create dummy input matching the classifier's expected input
    # Shape: (1, 16, 96) — the livekit-wakeword crate passes this exact shape
    dummy_input = torch.randn(1, MIN_EMBEDDINGS, EMBEDDING_DIM)

    # Export with named inputs/outputs matching what the Rust crate expects
    model.eval()
    torch.onnx.export(
        model,
        dummy_input,
        str(output_path),
        input_names=["embeddings"],   # Must match Rust code: ort::inputs!["embeddings" => tensor]
        output_names=["score"],       # Must match Rust code: outputs["score"]
        dynamic_axes={
            "embeddings": {0: "batch_size"},  # Allow batch dimension to vary
        },
        opset_version=17,
    )

    file_size = output_path.stat().st_size
    print(f"\n✅ Model exported: {output_path}")
    print(f"   File size: {file_size / 1024:.1f} KB")
    print(f"   Input: 'embeddings' (1, {MIN_EMBEDDINGS}, {EMBEDDING_DIM})")
    print(f"   Output: 'score' (1,)")
    print()
    print(f"To use in Handy:")
    print(f"  1. Copy to: src-tauri/resources/models/{filename}")
    print(f"  2. Update preload_wakeword() path in audio.rs if filename changed")
    print(f"  3. Uncomment preload_wakeword() call in AudioRecordingManager::new()")
    print(f"  4. Rebuild: bun run tauri dev")

    return output_path


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
def main():
    parser = argparse.ArgumentParser(
        description="Train a custom wake-word classifier for Handy",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    subparsers = parser.add_subparsers(dest="command", help="Command to run")

    # Record command
    rec_parser = subparsers.add_parser("record", help="Record audio samples")
    rec_parser.add_argument("--label", choices=["positive", "negative"], required=True,
                           help="positive=wake phrase, negative=other audio")
    rec_parser.add_argument("--count", type=int, default=30,
                           help="Number of samples to record (default: 30)")
    rec_parser.add_argument("--duration", type=float, default=2.0,
                           help="Duration of each sample in seconds (default: 2.0)")

    # Train command
    train_parser = subparsers.add_parser("train", help="Train the classifier")
    train_parser.add_argument("--wake-phrase", type=str, default="hey handy",
                             help="Your wake phrase (used for filename)")
    train_parser.add_argument("--epochs", type=int, default=100,
                             help="Training epochs (default: 100)")
    train_parser.add_argument("--lr", type=float, default=0.001,
                             help="Learning rate (default: 0.001)")

    # Extract command (test feature extraction on a single file)
    ext_parser = subparsers.add_parser("extract", help="Test feature extraction")
    ext_parser.add_argument("file", help="Path to WAV or NPY file")

    # Test command (test the trained model on a recording)
    test_parser = subparsers.add_parser("test", help="Test trained model on live audio")
    test_parser.add_argument("--model", type=str, required=True,
                            help="Path to trained ONNX model")
    test_parser.add_argument("--threshold", type=float, default=0.5,
                            help="Detection threshold (default: 0.5)")

    args = parser.parse_args()

    if args.command == "record":
        record_samples(args.label, args.count, args.duration)

    elif args.command == "train":
        print("=" * 60)
        print("  Wake-Word Classifier Training")
        print("=" * 60)
        print()

        extractor = FeatureExtractor()
        positive_data, negative_data = collect_dataset(extractor)
        model = train_classifier(positive_data, negative_data,
                                args.wake_phrase, args.epochs, args.lr)
        export_onnx(model, args.wake_phrase)

    elif args.command == "extract":
        extractor = FeatureExtractor()
        emb = extractor.extract_from_file(args.file)
        if emb is not None:
            print(f"Embedding sequence shape: {emb.shape}")
            print(f"Mean: {emb.mean():.4f}, Std: {emb.std():.4f}")
        else:
            print("Audio too short to extract embeddings")

    elif args.command == "test":
        test_model(args.model, args.threshold)

    else:
        parser.print_help()


def test_model(model_path: str, threshold: float = 0.5):
    """Test a trained model on live microphone audio."""
    try:
        import onnxruntime as ort
        import sounddevice as sd
    except ImportError:
        print("ERROR: Install onnxruntime and sounddevice")
        sys.exit(1)

    if not os.path.exists(model_path):
        print(f"ERROR: Model not found: {model_path}")
        sys.exit(1)

    extractor = FeatureExtractor()
    session = ort.InferenceSession(model_path, providers=["CPUExecutionProvider"])

    print(f"Testing model: {model_path}")
    print(f"Threshold: {threshold}")
    print(f"Listening for wake word... (Ctrl+C to stop)")
    print()

    chunk_duration = 2.0  # seconds
    chunk_samples = int(chunk_duration * SAMPLE_RATE)

    try:
        while True:
            audio = sd.rec(chunk_samples, samplerate=SAMPLE_RATE, channels=1, dtype="float32")
            sd.wait()
            audio = audio.flatten()

            emb = extractor.extract_embeddings(audio)
            if emb is None:
                continue

            # Run classifier
            emb_input = emb.reshape(1, MIN_EMBEDDINGS, EMBEDDING_DIM).astype(np.float32)
            result = session.run(None, {"embeddings": emb_input})
            score = float(result[0].flatten()[0])

            status = "🔔 DETECTED!" if score >= threshold else "  "
            bar = "█" * int(score * 40)
            print(f"  Score: {score:.3f} [{bar:<40s}] {status}")

    except KeyboardInterrupt:
        print("\nStopped.")


if __name__ == "__main__":
    main()