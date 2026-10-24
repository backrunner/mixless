# Synthetic audio fixtures

These files contain one second of generated stereo sine waves at 48 kHz:
440 Hz in the left channel and 880 Hz in the right channel, with amplitude
10000 in signed 16-bit PCM. They contain no acquired music or personal data and
are provided under the repository's MPL-2.0 license.

The reference encodings were generated with FFmpeg 9.0.2 from
`sample[channel, i] = trunc(10000 * sin(2*pi*(440*(channel+1))*i/48000))`:

| File | Reference encoder settings | SHA-256 |
| --- | --- | --- |
| tone-opus.webm | `-c:a libopus -b:a 96k` | `259b2996137cefc994a4cb80039e5f9f989e8b7fa33ac5028903c5c772d47b97` |
| tone-aac.ts | `-c:a aac -b:a 128k -f mpegts` | `5368803dbf54107aae43fa12f726af06c8eff464d3a33fd5afdc7f437ce7ba6b` |
| tone-aac.aac | `-c:a aac -b:a 128k -f adts` | `2750c921b78aa55cdd43005f5512fa7052ef5464abd93d5218390fce8b1dc8b2` |

Ordinary tests read these fixtures and generate WAV inputs in Rust. They never
invoke an external encoder, decoder or probe. Assertions use mixless's separate
Symphonia decoder to verify duration, sample rate and channel-specific signals.
