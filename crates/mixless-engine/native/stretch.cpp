#include <signalsmith-stretch.h>
#include <new>

struct InputView {
    const float *samples;
    struct Channel {
        const float *samples;
        float operator[](int offset) const { return samples[offset * 2]; }
    };
    Channel operator[](int channel) const { return {samples + channel}; }
};

struct OutputView {
    float *samples;
    struct Channel {
        float *samples;
        float &operator[](int offset) { return samples[offset * 2]; }
    };
    Channel operator[](int channel) const { return {samples + channel}; }
};

using Stretch = signalsmith::stretch::SignalsmithStretch<float>;

extern "C" {
void *mixless_stretch_create(int block, int interval) {
    try {
        auto *stretch = new Stretch(0);
        try { stretch->configure(2, block, interval, true); }
        catch (...) { delete stretch; throw; }
        return stretch;
    } catch (...) { return nullptr; }
}
void mixless_stretch_destroy(void *handle) { delete static_cast<Stretch *>(handle); }
void mixless_stretch_reset(void *handle) { static_cast<Stretch *>(handle)->reset(); }
int mixless_stretch_input_latency(void *handle) { return static_cast<Stretch *>(handle)->inputLatency(); }
int mixless_stretch_output_latency(void *handle) { return static_cast<Stretch *>(handle)->outputLatency(); }
void mixless_stretch_pitch(void *handle, float semitones) {
    static_cast<Stretch *>(handle)->setTransposeSemitones(semitones);
}
void mixless_stretch_seek(void *handle, const float *input, int frames, double rate) {
    static_cast<Stretch *>(handle)->seek(InputView{input}, frames, rate);
}
void mixless_stretch_process(void *handle, const float *input, int input_frames, float *output, int output_frames) {
    static_cast<Stretch *>(handle)->process(InputView{input}, input_frames, OutputView{output}, output_frames);
}
}
