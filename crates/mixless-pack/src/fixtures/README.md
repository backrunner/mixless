# Stem codec fixtures

`stem.flac` and `stem.mp3` are synthetic 0.1-second stereo signals at 44.1 kHz
with every float sample set to 0.05. They contain no acquired audio.
They use mixless's production FLAC 24-bit and MP3 320 kbps encoders.

Regenerate with:

```sh
cargo test --locked -p mixless-stems generate_portable_fixtures -- --ignored
```
