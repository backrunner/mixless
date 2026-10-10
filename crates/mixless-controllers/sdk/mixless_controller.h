/* SPDX-License-Identifier: MPL-2.0
 * mixless controller ABI 1. Independent of third-party DJ application APIs.
 * Compile for wasm32 without WASI or libc. All imports queue validated effects.
 */
#ifndef MIXLESS_CONTROLLER_H
#define MIXLESS_CONTROLLER_H
#include <stdint.h>
#define MIXLESS_IMPORT(name) __attribute__((import_module("mixless"), import_name(name)))
#define MIXLESS_EXPORT(name) __attribute__((export_name(name)))
enum mixless_event_kind { CONNECT, INPUT, STATE, TIMER, DISCONNECT, FEATURE, IO_ERROR };
enum mixless_value_kind { PRESS, HOLD, RELEASE, NORMALIZED, RELATIVE, JOG_TURNS };
enum mixless_report_kind { OUTPUT, FEATURE_WRITE };
MIXLESS_IMPORT("emit") void mixless_emit(int control, int kind, double value);
MIXLESS_IMPORT("value") double mixless_value(int control);
MIXLESS_IMPORT("meter") double mixless_meter(int bus, int channel);
MIXLESS_IMPORT("send") void mixless_send(int endpoint, int kind, const uint8_t *data, int length);
MIXLESS_IMPORT("read_feature") void mixless_read_feature(int endpoint, int report, int length);
MIXLESS_IMPORT("timer") void mixless_timer(int delay_ms);
/* Provide these two exports. The input buffer must reserve at least 4096 bytes. */
MIXLESS_EXPORT("mixless_buffer") uint8_t *mixless_buffer(void);
MIXLESS_EXPORT("mixless_event") int mixless_event(int kind, int endpoint, const uint8_t *data, int length, int64_t time_ms);
#endif
