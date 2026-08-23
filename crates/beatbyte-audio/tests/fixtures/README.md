# Decode fixtures

A half-second synthesized tone (440 Hz + 660 Hz overtone, attack/decay
envelope) — **fully original audio**, generated for these tests; no
copyrighted material. Recreate from scratch:

```bash
# tone.wav — python stdlib (22.05 kHz mono 16-bit, 0.5 s)
python3 - <<'PY'
import wave, math, struct
rate = 22050; frames = int(rate * 0.5); out = []
for i in range(frames):
    t = i / rate
    env = min(1.0, t / 0.01) * math.exp(-t * 2.0)
    v = 0.6*math.sin(2*math.pi*440*t) + 0.25*math.sin(2*math.pi*660*t)
    out.append(int(max(-1, min(1, v*env)) * 32767))
w = wave.open("tone.wav", "wb")
w.setnchannels(1); w.setsampwidth(2); w.setframerate(rate)
w.writeframes(struct.pack(f"<{frames}h", *out)); w.close()
PY

ffmpeg -i tone.wav -ac 2 -c:a vorbis -strict experimental tone.ogg  # stereo: also tests downmix
flac -o tone.flac tone.wav
lame -b 64 tone.wav tone.mp3
afconvert -f m4af -d aac tone.wav tone.m4a   # macOS
```
