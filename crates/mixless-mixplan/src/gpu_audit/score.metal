// Mirror composition::cost's complete score and rejection rules. This kernel
// is diagnostic only: its output never selects, caches or rejects a plan.
// Frame stride (40 floats): u, sec, voices[2], bands[2][4][3],
// full[2], melody[2], vocal[2], foreground[2], weight[2], dt, sounding mask.
// Source-only reductions are supplied by the CPU's immutable Prepared data.
#include <metal_stdlib>
using namespace metal;
struct Query {
    uint offset, count, tonality, reserved;
    float shape[16];
};
struct Score {
    uint valid;
    float cost;
};
float fade(device const float *shape, uint d, uint s, float u) {
    uint k = d * 8 + s * 2;
    float start = shape[k], end = shape[k + 1];
    if (end <= start)
        return float(u >= end);
    float x = clamp((u - start) / (end - start), 0.0f, 1.0f);
    return x * x * x * (10.0f + x * (-15.0f + 6.0f * x));
}
kernel void score_batch(device const float *frames [[buffer(0)]],
                        device const Query *queries [[buffer(1)]],
                        device Score *output [[buffer(2)]], constant uint &count [[buffer(3)]],
                        uint index [[thread_position_in_grid]]) {
    if (index >= count)
        return;
    device const Query &q = queries[index];
    float cost = 0.0f, absent = 0.0f, previous = 0.0f, shared = 0.0f;
    output[index] = {0, 0.0f};
    for (uint i = 0; i < q.count; i++) {
        device const float *f = frames + (q.offset + i) * 40;
        float g[2][4];
        for (uint d = 0; d < 2; d++)
            for (uint s = 0; s < 4; s++) {
                float x = fade(q.shape, d, s, f[0]);
                g[d][s] = d == 0 ? 1.0f - x : x;
            }
        if (q.tonality != 0) {
            uint sounding = uint(f[39]);
            float tonal[2] = {0.0f, 0.0f};
            for (uint d = 0; d < 2; d++)
                for (uint s = 0; s < 4; s++)
                    if (s != 1 && (sounding & (1u << (d * 4 + s))))
                        tonal[d] = max(tonal[d], g[d][s]);
            if ((sounding & 1u) && (sounding & 16u) && g[0][0] * g[1][0] > 0.0001f)
                return;
            float floor = q.tonality == 1 ? 0.25f : 0.01f;
            if (tonal[0] > floor && tonal[1] > floor) {
                shared += f[0] - previous;
                if (shared > 1.0f + 1e-4f)
                    return;
            }
        }
        if (min(f[2], f[3]) > 0.35f && g[0][0] * g[1][0] > 0.01f)
            return;
        float p[2][3];
        for (uint d = 0; d < 2; d++) {
            float h = 20.0f * precise::pow(12.5f, (1.0f - g[d][0]) * (1.0f - g[d][2]) *
                                                      (1.0f - g[d][3]) * g[d][1] * g[1 - d][3]);
            for (uint b = 0; b < 3; b++) {
                float sum = 0.0f;
                for (uint s = 0; s < 4; s++)
                    sum += f[4 + d * 12 + s * 3 + b] * (g[d][s] * g[d][s]);
                float z = h / (b == 0 ? 70.0f : (b == 1 ? 600.0f : 5000.0f));
                p[d][b] = sum / (1.0f + (z * z) * (z * z));
            }
        }
        float progress = g[1][2];
        float target = max(f[28] * (1.0f - progress) + f[29] * progress, 1e-12f);
        float heard = 0.0f;
        for (uint d = 0; d < 2; d++)
            for (uint b = 0; b < 3; b++)
                heard += p[d][b];
        heard = max(heard, 1e-12f);
        float available = max(f[30], f[31]);
        float bed = f[30] * (g[0][2] * g[0][2]) + f[31] * (g[1][2] * g[1][2]);
        float voice = 0.0f;
        for (uint d = 0; d < 2; d++)
            voice += f[32 + d] * (g[d][0] * g[d][0]);
        float expected = f[34] * (1.0f - progress) + f[35] * progress;
        if ((available > (f[28] + f[29]) * 0.05f && bed < available * 0.0001f &&
             voice < available * 0.125f) ||
            (expected > (f[28] + f[29]) * 0.05f && bed + voice < expected * 0.25f)) {
            absent += f[0] - previous;
            if (absent >= 1.0f)
                return;
        } else
            absent = 0.0f;
        previous = f[0];
        float delta = 10.0f * precise::log10(heard / target);
        if (delta < -6.0f)
            return;
        float lo = min(delta, 0.0f), hi = max(delta, 0.0f);
        cost += lo * lo + 0.2f * (hi * hi);
        for (uint b = 1; b < 3; b++)
            cost += 4.0f * min(p[0][b], p[1][b]) / max(p[0][b] + p[1][b], 1e-12f);
    }
    for (uint i = 1; i < q.count; i++) {
        device const float *f = frames + (q.offset + i) * 40;
        device const float *old = f - 40;
        for (uint d = 0; d < 2; d++) {
            float before = fade(q.shape, d, 2, old[0]), after = fade(q.shape, d, 2, f[0]);
            if (d == 0) {
                before = 1.0f - before;
                after = 1.0f - after;
            }
            float slope = abs(after - before) / f[38];
            cost += 16.0f * f[36 + d] * (slope * slope);
        }
    }
    output[index] = {1, cost / float(q.count)};
}
