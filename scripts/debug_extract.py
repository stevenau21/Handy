import numpy as np
import soundfile as sf
import onnxruntime as ort

MEL_ONNX = 'src-tauri/vendor/livekit-wakeword/onnx/melspectrogram.onnx'
EMB_ONNX = 'src-tauri/vendor/livekit-wakeword/onnx/embedding_model.onnx'

print("Loading models...")
mel_session = ort.InferenceSession(MEL_ONNX, providers=['CPUExecutionProvider'])
emb_session = ort.InferenceSession(EMB_ONNX, providers=['CPUExecutionProvider'])
print("Models loaded.")

print("Reading audio...")
audio, sr = sf.read('scripts/wakeword_data/positive/positive_0001.wav')
print(f'Audio shape: {audio.shape}, SR: {sr}')

audio_f32 = audio.flatten().astype(np.float32)
mel_input = audio_f32.reshape(1, -1)
print(f'Mel input shape: {mel_input.shape}')

print("Running mel model...")
mel_result = mel_session.run(None, {mel_session.get_inputs()[0].name: mel_input})
mel = mel_result[0]
print(f'Mel output shape: {mel.shape}')

if mel.shape[0] >= 76:
    window = mel[0:76, :]
    print(f'Window shape: {window.shape}')

    emb_input = window.reshape(1, 76, 32, 1).astype(np.float32)
    print(f'Emb input shape: {emb_input.shape}')

    print("Running emb model...")
    emb_result = emb_session.run(None, {emb_session.get_inputs()[0].name: emb_input})
    emb = emb_result[0]
    print(f'Emb output shape: {emb.shape}')
else:
    print(f"Mel output too short: {mel.shape[0]} frames < 76")