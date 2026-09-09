// Offline-only system classifier. No devices, network, or model downloads.
#import <Foundation/Foundation.h>
#import <AVFoundation/AVFoundation.h>
#import <SoundAnalysis/SoundAnalysis.h>
#include <math.h>
#include <stdint.h>
#include <string.h>

typedef struct { double start_sec, end_sec, confidence; } MixlessVoiceInterval;

@interface MixlessVoiceObserver : NSObject <SNResultsObserving>
@property MixlessVoiceInterval *output;
@property uint32_t capacity;
@property uint32_t count;
@property BOOL failed;
@end

@implementation MixlessVoiceObserver
- (void)request:(id<SNRequest>)request didProduceResult:(id<SNResult>)result {
    @synchronized (self) {
        if (![result isKindOfClass:SNClassificationResult.class] || self.count >= self.capacity) {
            self.failed = YES;
            return;
        }
        SNClassificationResult *r = (SNClassificationResult *)result;
        double voice = 0.;
        // Exact human-voice labels exclude singing bowls and bird vocalizations.
        for (SNClassification *c in r.classifications) {
            NSString *s = c.identifier;
            if ([s isEqualToString:@"singing"] || [s isEqualToString:@"speech"] ||
                [s isEqualToString:@"rapping"] || [s isEqualToString:@"choir_singing"] ||
                [s isEqualToString:@"humming"]) {
                voice = fmax(voice, c.confidence);
            }
        }
        // Emit low scores as well. Coverage accounting must distinguish a
        // completed negative prediction from a missing/failed model window.
        // Rust never uses a low voice score to clear the DSP foreground risk.
        double start = CMTimeGetSeconds(r.timeRange.start);
        double end = CMTimeGetSeconds(CMTimeRangeGetEnd(r.timeRange));
        if (!isfinite(start) || !isfinite(end) || !isfinite(voice) || start < 0. || end <= start) {
            self.failed = YES;
            return;
        }
        self.output[self.count++] = (MixlessVoiceInterval){start, end, fmin(1., fmax(0., voice))};
    }
}
- (void)request:(id<SNRequest>)request didFailWithError:(NSError *)error {
    @synchronized (self) { self.failed = YES; }
}
- (void)requestDidComplete:(id<SNRequest>)request {}
@end

// Count, -1 unavailable/failed, -2 budget exhausted. Deadline checks happen
// between half-second buffers, not by preempting execution inside Core ML.
int32_t mixless_voice_analyze(const float *stereo, size_t frames, uint32_t sr,
                             MixlessVoiceInterval *output, uint32_t capacity,
                             double budget_seconds) {
    if (!stereo || !output || !frames || !sr || !capacity || budget_seconds <= 0.) return -1;
    @autoreleasepool {
        SNAudioStreamAnalyzer *analyzer = nil;
        @try {
            if (@available(macOS 12.0, *)) {
                double deadline = NSProcessInfo.processInfo.systemUptime + budget_seconds;
                NSError *error = nil;
                SNClassifySoundRequest *request = [[SNClassifySoundRequest alloc]
                    initWithClassifierIdentifier:SNClassifierIdentifierVersion1 error:&error];
                if (!request) return -1;
                request.overlapFactor = 0.;
                request.windowDuration = CMTimeMakeWithSeconds(1.5, 48000);
                AVAudioFormat *format = [[AVAudioFormat alloc] initStandardFormatWithSampleRate:sr channels:1];
                analyzer = [[SNAudioStreamAnalyzer alloc] initWithFormat:format];
                MixlessVoiceObserver *observer = [MixlessVoiceObserver new];
                observer.output = output; observer.capacity = capacity;
                if (![analyzer addRequest:request withObserver:observer error:&error]) return -1;
                uint32_t chunk = (sr + 1) / 2;
                AVAudioPCMBuffer *buffer = [[AVAudioPCMBuffer alloc] initWithPCMFormat:format frameCapacity:chunk];
                if (!buffer || !buffer.floatChannelData) return -1;
                int32_t status = 0;
                for (size_t offset = 0; offset < frames; offset += chunk) {
                    @autoreleasepool {
                        if (NSProcessInfo.processInfo.systemUptime >= deadline) { status = -2; break; }
                        uint32_t count = (uint32_t)MIN((size_t)chunk, frames-offset);
                        buffer.frameLength = count;
                        float *mono = buffer.floatChannelData[0];
                        double left = 0., right = 0., combined = 0.;
                        for (uint32_t j = 0; j < count; j++) {
                            float l = stereo[2*(offset+j)], r = stereo[2*(offset+j)+1];
                            mono[j] = (l+r)*0.5f;
                            left += l*l; right += r*r; combined += mono[j]*mono[j];
                        }
                        if (combined < fmax(left,right)*0.01) {
                            size_t channel = left >= right ? 0 : 1;
                            for (uint32_t j=0; j<count; j++) mono[j]=stereo[2*(offset+j)+channel];
                        }
                        [analyzer analyzeAudioBuffer:buffer atAudioFramePosition:(AVAudioFramePosition)offset];
                    }
                }
                if (status == 0) [analyzer completeAnalysis];
                [analyzer removeAllRequests];
                @synchronized (observer) {
                    if (observer.failed) return -1;
                    if (NSProcessInfo.processInfo.systemUptime > deadline) return -2;
                    return status < 0 ? status : (int32_t)observer.count;
                }
            }
            return -1;
        } @catch (NSException *exception) {
            return -1;
        } @finally {
            // No callbacks may outlive the caller-owned Rust output buffer.
            [analyzer removeAllRequests];
        }
    }
}
