/* SPDX-License-Identifier: MPL-2.0
 * Fictional protocol invented for mixless tests, not a mapping for a real product.
 * Report 1: ID, sequence, buttons, wrapping platter counter, checksum (sum mod 256).
 * Feature 3 enables the device. Feature 4 returns readiness. Output 2 sets the LED.
 */
#include "../../sdk/mixless_controller.h"
static uint8_t input[4096];
static uint8_t ready, buttons, sequence, counter, have_counter, have_sequence;
static uint8_t led = 255, retries;

uint8_t *mixless_buffer(void) { return input; }
static void set_led(uint8_t value) {
    if (value == led) return;
    uint8_t report[] = {2, value, (uint8_t)(2 + value)};
    mixless_send(0, OUTPUT, report, sizeof(report));
    led = value;
}
int mixless_event(int kind, int endpoint, const uint8_t *data, int length, int64_t time_ms) {
    (void)time_ms;
    if (kind == CONNECT) {
        uint8_t enable[] = {3, 1};
        mixless_send(0, FEATURE_WRITE, enable, sizeof(enable));
        mixless_read_feature(0, 4, 2);
        mixless_timer(100);
    } else if (kind == FEATURE && endpoint == 0 && length == 2 && data[0] == 4) {
        ready = data[1] == 1;
        if (ready) set_led(mixless_value(0) > 0.5);
    } else if (kind == TIMER && !ready) {
        if (++retries > 3) return 1;
        mixless_read_feature(0, 4, 2);
        mixless_timer(100);
    } else if (kind == STATE && ready) {
        set_led(mixless_value(0) > 0.5);
    } else if (kind == INPUT && ready && endpoint == 0 && length == 5 && data[0] == 1) {
        if ((uint8_t)(data[0] + data[1] + data[2] + data[3]) != data[4]) return 0;
        if (have_sequence && sequence == data[1]) return 0;
        have_sequence = 1;
        sequence = data[1];
        if ((data[2] & 1) && !(buttons & 1)) mixless_emit(0, PRESS, 1);
        if ((data[2] ^ buttons) & 2) mixless_emit(1, (data[2] & 2) ? HOLD : RELEASE, 0);
        if (have_counter) {
            int8_t delta = (int8_t)(data[3] - counter);
            if (delta) mixless_emit(2, JOG_TURNS, delta / 1024.0);
        }
        have_counter = 1;
        counter = data[3];
        buttons = data[2];
    } else if (kind == DISCONNECT) {
        if (buttons & 2) mixless_emit(1, RELEASE, 0);
        set_led(0);
    } else if (kind == IO_ERROR) {
        return 2;
    }
    return 0;
}
