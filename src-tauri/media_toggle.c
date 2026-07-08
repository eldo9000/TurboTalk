// macOS media key toggle + sample-energy playback detection.
// Compiled into the binary — same process, same permissions.
//
// Documented-correct idiom: use CoreAudio process taps (CATapDescription +
// AudioHardwareCreateProcessTap) to observe actual system output samples.
// MediaRemote/Now Playing and CoreAudio "is running" flags are state heuristics;
// they do not distinguish real playback from warm or stale audio pipelines.

#import <Cocoa/Cocoa.h>
#import <CoreAudio/CoreAudio.h>
#import <CoreAudio/CATapDescription.h>
#import <CoreAudio/AudioHardwareTapping.h>
#include <errno.h>
#include <math.h>
#include <pthread.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

static UInt64 g_last_probe_samples = 0;
static double g_last_probe_rms = 0.0;
static float g_last_probe_peak = 0.0f;
static int g_last_probe_status = -99;
static char g_last_probe_diag[1024] = "not-run";

void media_toggle_play_pause(void) {
    @autoreleasepool {
        int keyCode = 16;
        NSEvent *down = [NSEvent otherEventWithType:NSEventTypeSystemDefined
                                           location:NSZeroPoint
                                      modifierFlags:0
                                          timestamp:0
                                       windowNumber:0
                                            context:nil
                                            subtype:8
                                              data1:((keyCode << 16) | (0xa << 8))
                                              data2:-1];
        CGEventRef cgDown = [down CGEvent];
        if (cgDown) CGEventPost(kCGSessionEventTap, cgDown);
        [NSThread sleepForTimeInterval:0.01];
        NSEvent *up = [NSEvent otherEventWithType:NSEventTypeSystemDefined
                                        location:NSZeroPoint
                                   modifierFlags:0
                                       timestamp:0
                                    windowNumber:0
                                         context:nil
                                         subtype:8
                                           data1:((keyCode << 16) | (0xb << 8))
                                           data2:-1];
        CGEventRef cgUp = [up CGEvent];
        if (cgUp) CGEventPost(kCGSessionEventTap, cgUp);
    }
}

typedef struct {
    pthread_mutex_t mutex;
    pthread_cond_t cond;
    double sum_squares;
    UInt64 samples;
    float peak;
    double input_sum_squares;
    UInt64 input_samples;
    float input_peak;
    UInt32 input_buffers;
    double output_sum_squares;
    UInt64 output_samples;
    float output_peak;
    UInt32 output_buffers;
    AudioStreamBasicDescription format;
} AudioProbeContext;

static void turbotalk_fourcc_to_cstr(UInt32 value, char out[5]) {
    out[0] = (char)((value >> 24) & 0xff);
    out[1] = (char)((value >> 16) & 0xff);
    out[2] = (char)((value >> 8) & 0xff);
    out[3] = (char)(value & 0xff);
    out[4] = '\0';
    for (int i = 0; i < 4; i++) {
        if (out[i] < 32 || out[i] > 126) out[i] = '?';
    }
}

static void turbotalk_format_diag(
    const AudioStreamBasicDescription *format,
    char *out,
    size_t out_len
) {
    char fourcc[5];
    turbotalk_fourcc_to_cstr(format->mFormatID, fourcc);
    snprintf(out,
             out_len,
             "fmt=%s rate=%.1f flags=0x%x bytes/frame=%u channels/frame=%u bits/channel=%u",
             fourcc,
             format->mSampleRate,
             format->mFormatFlags,
             format->mBytesPerFrame,
             format->mChannelsPerFrame,
             format->mBitsPerChannel);
}

static void turbotalk_accumulate_buffer_list(
    const AudioBufferList *buffer_list,
    const AudioStreamBasicDescription *format,
    UInt64 *sample_count,
    double *sum_squares,
    float *peak,
    UInt32 *buffer_count
) {
    if (!buffer_list) return;

    *buffer_count += buffer_list->mNumberBuffers;

    bool is_pcm = format->mFormatID == kAudioFormatLinearPCM;
    bool is_float = (format->mFormatFlags & kAudioFormatFlagIsFloat) != 0;
    bool is_signed_integer = (format->mFormatFlags & kAudioFormatFlagIsSignedInteger) != 0;
    bool is_non_interleaved = (format->mFormatFlags & kAudioFormatFlagIsNonInterleaved) != 0;
    UInt32 channels = format->mChannelsPerFrame == 0 ? 1 : format->mChannelsPerFrame;
    UInt32 bytes_per_sample = format->mBitsPerChannel / 8;

    for (UInt32 buffer_index = 0; buffer_index < buffer_list->mNumberBuffers; buffer_index++) {
        const AudioBuffer *buffer = &buffer_list->mBuffers[buffer_index];
        if (!buffer->mData || buffer->mDataByteSize == 0) continue;

        UInt32 samples_in_buffer = 0;
        if (is_non_interleaved || format->mBytesPerFrame == 0) {
            samples_in_buffer = bytes_per_sample == 0 ? 0 : buffer->mDataByteSize / bytes_per_sample;
        } else {
            UInt32 frames = buffer->mDataByteSize / format->mBytesPerFrame;
            samples_in_buffer = frames * channels;
        }

        if (!is_pcm || bytes_per_sample == 0 || samples_in_buffer == 0) {
            const float *samples = (const float *)buffer->mData;
            UInt32 count = buffer->mDataByteSize / sizeof(float);
            for (UInt32 i = 0; i < count; i++) {
                float sample = samples[i];
                float abs_sample = fabsf(sample);
                if (abs_sample > *peak) *peak = abs_sample;
                *sum_squares += (double)sample * (double)sample;
            }
            *sample_count += count;
            continue;
        }

        if (is_float && bytes_per_sample == sizeof(float)) {
            const float *samples = (const float *)buffer->mData;
            for (UInt32 i = 0; i < samples_in_buffer; i++) {
                float sample = samples[i];
                float abs_sample = fabsf(sample);
                if (abs_sample > *peak) *peak = abs_sample;
                *sum_squares += (double)sample * (double)sample;
            }
            *sample_count += samples_in_buffer;
        } else if (is_float && bytes_per_sample == sizeof(double)) {
            const double *samples = (const double *)buffer->mData;
            for (UInt32 i = 0; i < samples_in_buffer; i++) {
                double sample = samples[i];
                float abs_sample = (float)fabs(sample);
                if (abs_sample > *peak) *peak = abs_sample;
                *sum_squares += sample * sample;
            }
            *sample_count += samples_in_buffer;
        } else if (is_signed_integer && bytes_per_sample == sizeof(int16_t)) {
            const int16_t *samples = (const int16_t *)buffer->mData;
            for (UInt32 i = 0; i < samples_in_buffer; i++) {
                float sample = (float)samples[i] / 32768.0f;
                float abs_sample = fabsf(sample);
                if (abs_sample > *peak) *peak = abs_sample;
                *sum_squares += (double)sample * (double)sample;
            }
            *sample_count += samples_in_buffer;
        } else if (is_signed_integer && bytes_per_sample == sizeof(int32_t)) {
            const int32_t *samples = (const int32_t *)buffer->mData;
            for (UInt32 i = 0; i < samples_in_buffer; i++) {
                float sample = (float)((double)samples[i] / 2147483648.0);
                float abs_sample = fabsf(sample);
                if (abs_sample > *peak) *peak = abs_sample;
                *sum_squares += (double)sample * (double)sample;
            }
            *sample_count += samples_in_buffer;
        } else {
            const float *samples = (const float *)buffer->mData;
            UInt32 count = buffer->mDataByteSize / sizeof(float);
            for (UInt32 i = 0; i < count; i++) {
                float sample = samples[i];
                float abs_sample = fabsf(sample);
                if (abs_sample > *peak) *peak = abs_sample;
                *sum_squares += (double)sample * (double)sample;
            }
            *sample_count += count;
        }
    }
}

static OSStatus turbotalk_audio_probe_io_proc(
    AudioObjectID inDevice,
    const AudioTimeStamp *inNow,
    const AudioBufferList *inInputData,
    const AudioTimeStamp *inInputTime,
    AudioBufferList *outOutputData,
    const AudioTimeStamp *inOutputTime,
    void *inClientData
) {
    (void)inDevice;
    (void)inNow;
    (void)inInputTime;
    (void)inOutputTime;

    AudioProbeContext *ctx = (AudioProbeContext *)inClientData;
    if (!ctx) return noErr;

    double input_sum_squares = 0.0;
    UInt64 input_sample_count = 0;
    float input_peak = 0.0f;
    UInt32 input_buffer_count = 0;
    turbotalk_accumulate_buffer_list(inInputData,
                                     &ctx->format,
                                     &input_sample_count,
                                     &input_sum_squares,
                                     &input_peak,
                                     &input_buffer_count);

    double output_sum_squares = 0.0;
    UInt64 output_sample_count = 0;
    float output_peak = 0.0f;
    UInt32 output_buffer_count = 0;
    turbotalk_accumulate_buffer_list(outOutputData,
                                     &ctx->format,
                                     &output_sample_count,
                                     &output_sum_squares,
                                     &output_peak,
                                     &output_buffer_count);

    UInt64 sample_count = input_sample_count + output_sample_count;
    if (sample_count == 0 && input_buffer_count == 0 && output_buffer_count == 0) return noErr;

    pthread_mutex_lock(&ctx->mutex);
    ctx->input_sum_squares += input_sum_squares;
    ctx->input_samples += input_sample_count;
    if (input_peak > ctx->input_peak) ctx->input_peak = input_peak;
    ctx->input_buffers += input_buffer_count;
    ctx->output_sum_squares += output_sum_squares;
    ctx->output_samples += output_sample_count;
    if (output_peak > ctx->output_peak) ctx->output_peak = output_peak;
    ctx->output_buffers += output_buffer_count;
    ctx->sum_squares += input_sum_squares + output_sum_squares;
    ctx->samples += sample_count;
    if (input_peak > ctx->peak) ctx->peak = input_peak;
    if (output_peak > ctx->peak) ctx->peak = output_peak;
    pthread_cond_signal(&ctx->cond);
    pthread_mutex_unlock(&ctx->mutex);

    return noErr;
}

static NSString *turbotalk_tap_uid(AudioObjectID tap_id) {
    AudioObjectPropertyAddress address = {
        kAudioTapPropertyUID,
        kAudioObjectPropertyScopeGlobal,
        kAudioObjectPropertyElementMain
    };
    CFStringRef uid = NULL;
    UInt32 size = sizeof(uid);
    OSStatus err = AudioObjectGetPropertyData(tap_id, &address, 0, NULL, &size, &uid);
    if (err != noErr || uid == NULL) return nil;
    return CFBridgingRelease(uid);
}

static void turbotalk_add_millis_to_timespec(struct timespec *ts, long millis) {
    ts->tv_sec += millis / 1000;
    ts->tv_nsec += (millis % 1000) * 1000000L;
    if (ts->tv_nsec >= 1000000000L) {
        ts->tv_sec += 1;
        ts->tv_nsec -= 1000000000L;
    }
}

static NSString *turbotalk_default_output_device_uid(void) {
    AudioObjectPropertyAddress default_address = {
        kAudioHardwarePropertyDefaultOutputDevice,
        kAudioObjectPropertyScopeGlobal,
        kAudioObjectPropertyElementMain
    };
    AudioDeviceID device_id = kAudioObjectUnknown;
    UInt32 size = sizeof(device_id);
    OSStatus err = AudioObjectGetPropertyData(
        kAudioObjectSystemObject,
        &default_address,
        0,
        NULL,
        &size,
        &device_id
    );
    if (err != noErr || device_id == kAudioObjectUnknown) {
        fprintf(stderr,
                "[media_control] default output device unavailable err=%d device=%u\n",
                (int)err,
                device_id);
        return nil;
    }

    AudioObjectPropertyAddress uid_address = {
        kAudioDevicePropertyDeviceUID,
        kAudioObjectPropertyScopeGlobal,
        kAudioObjectPropertyElementMain
    };
    CFStringRef uid = NULL;
    size = sizeof(uid);
    err = AudioObjectGetPropertyData(device_id, &uid_address, 0, NULL, &size, &uid);
    if (err != noErr || uid == NULL) {
        fprintf(stderr,
                "[media_control] default output UID unavailable err=%d device=%u\n",
                (int)err,
                device_id);
        return nil;
    }

    return CFBridgingRelease(uid);
}

static AudioDeviceID turbotalk_default_output_device_id(void) {
    AudioObjectPropertyAddress default_address = {
        kAudioHardwarePropertyDefaultOutputDevice,
        kAudioObjectPropertyScopeGlobal,
        kAudioObjectPropertyElementMain
    };
    AudioDeviceID device_id = kAudioObjectUnknown;
    UInt32 size = sizeof(device_id);
    OSStatus err = AudioObjectGetPropertyData(
        kAudioObjectSystemObject,
        &default_address,
        0,
        NULL,
        &size,
        &device_id
    );
    if (err != noErr) return kAudioObjectUnknown;
    return device_id;
}

static void turbotalk_object_name(AudioObjectID object_id, char *out, size_t out_len) {
    AudioObjectPropertyAddress address = {
        kAudioObjectPropertyName,
        kAudioObjectPropertyScopeGlobal,
        kAudioObjectPropertyElementMain
    };
    CFStringRef name = NULL;
    UInt32 size = sizeof(name);
    OSStatus err = AudioObjectGetPropertyData(object_id, &address, 0, NULL, &size, &name);
    if (err != noErr || name == NULL) {
        snprintf(out, out_len, "name-err=%d", (int)err);
        return;
    }

    NSString *ns_name = CFBridgingRelease(name);
    snprintf(out, out_len, "%s", ns_name.UTF8String ?: "(non-utf8)");
}

static OSStatus turbotalk_tap_format(AudioObjectID tap_id, AudioStreamBasicDescription *format) {
    AudioObjectPropertyAddress address = {
        kAudioTapPropertyFormat,
        kAudioObjectPropertyScopeGlobal,
        kAudioObjectPropertyElementMain
    };
    UInt32 size = sizeof(*format);
    return AudioObjectGetPropertyData(tap_id, &address, 0, NULL, &size, format);
}

static void turbotalk_first_stream_format(
    AudioObjectID device_id,
    AudioObjectPropertyScope scope,
    char *out,
    size_t out_len
) {
    AudioObjectPropertyAddress streams_address = {
        kAudioDevicePropertyStreams,
        scope,
        kAudioObjectPropertyElementMain
    };
    UInt32 size = 0;
    OSStatus err = AudioObjectGetPropertyDataSize(device_id, &streams_address, 0, NULL, &size);
    if (err != noErr || size < sizeof(AudioStreamID)) {
        snprintf(out, out_len, "streams err=%d size=%u", (int)err, size);
        return;
    }

    AudioStreamID streams[16];
    UInt32 stream_count = size / sizeof(AudioStreamID);
    if (stream_count > 16) stream_count = 16;
    size = stream_count * sizeof(AudioStreamID);
    err = AudioObjectGetPropertyData(device_id, &streams_address, 0, NULL, &size, streams);
    if (err != noErr || stream_count == 0) {
        snprintf(out, out_len, "streams read err=%d count=%u", (int)err, stream_count);
        return;
    }

    AudioObjectPropertyAddress format_address = {
        kAudioStreamPropertyVirtualFormat,
        kAudioObjectPropertyScopeGlobal,
        kAudioObjectPropertyElementMain
    };
    AudioStreamBasicDescription format;
    size = sizeof(format);
    err = AudioObjectGetPropertyData(streams[0], &format_address, 0, NULL, &size, &format);
    if (err != noErr) {
        snprintf(out, out_len, "stream format err=%d count=%u", (int)err, stream_count);
        return;
    }

    char format_diag[256];
    turbotalk_format_diag(&format, format_diag, sizeof(format_diag));
    snprintf(out, out_len, "streams=%u first=%u %s", stream_count, streams[0], format_diag);
}

static int turbotalk_audio_is_playing_with_process_tap(void) {
    g_last_probe_samples = 0;
    g_last_probe_rms = 0.0;
    g_last_probe_peak = 0.0f;
    g_last_probe_status = -99;

    if (@available(macOS 14.2, *)) {
        @autoreleasepool {
            AudioObjectID tap_id = kAudioObjectUnknown;
            AudioObjectID aggregate_id = kAudioObjectUnknown;
            AudioDeviceIOProcID io_proc_id = NULL;
            int result = -1;

            NSString *device_uid = turbotalk_default_output_device_uid();
            CATapDescription *tap_description = nil;
            if (device_uid) {
                tap_description =
                    [[CATapDescription alloc] initExcludingProcesses:@[]
                                                        andDeviceUID:device_uid
                                                          withStream:0];
            }
            if (!tap_description) {
                tap_description =
                    [[CATapDescription alloc] initMonoGlobalTapButExcludeProcesses:@[]];
            }
            tap_description.name = @"TurboTalk playback probe";
            tap_description.UUID = [NSUUID UUID];
            tap_description.privateTap = YES;
            tap_description.muteBehavior = CATapUnmuted;
            tap_description.mixdown = YES;
            tap_description.mono = YES;

            OSStatus err = AudioHardwareCreateProcessTap(tap_description, &tap_id);
            if (err != noErr || tap_id == kAudioObjectUnknown) {
                g_last_probe_status = (int)err;
                fprintf(stderr,
                        "[media_control] process tap unavailable err=%d tap=%u\n",
                        (int)err,
                        tap_id);
                return -1;
            }

            AudioStreamBasicDescription tap_format;
            memset(&tap_format, 0, sizeof(tap_format));
            err = turbotalk_tap_format(tap_id, &tap_format);
            if (err != noErr) {
                g_last_probe_status = (int)err;
                snprintf(g_last_probe_diag,
                         sizeof(g_last_probe_diag),
                         "tap-format-unavailable err=%d tap=%u",
                         (int)err,
                         tap_id);
                fprintf(stderr, "[media_control] %s\n", g_last_probe_diag);
                AudioHardwareDestroyProcessTap(tap_id);
                return -1;
            }

            char tap_format_diag[256];
            turbotalk_format_diag(&tap_format, tap_format_diag, sizeof(tap_format_diag));

            NSString *tap_uid = turbotalk_tap_uid(tap_id);
            if (!tap_uid) tap_uid = tap_description.UUID.UUIDString;
            if (!tap_uid) {
                g_last_probe_status = -2;
                fprintf(stderr, "[media_control] process tap has no UID\n");
                AudioHardwareDestroyProcessTap(tap_id);
                return -1;
            }

            NSString *aggregate_uid =
                [NSString stringWithFormat:@"com.turbotalk.playback-probe.%@",
                                           [[NSUUID UUID] UUIDString]];
            NSDictionary *tap_entry = @{ @kAudioSubTapUIDKey: tap_uid };
            NSDictionary *aggregate_description = @{
                @kAudioAggregateDeviceNameKey: @"TurboTalk Playback Probe",
                @kAudioAggregateDeviceUIDKey: aggregate_uid,
                @kAudioAggregateDeviceIsPrivateKey: @YES,
                @kAudioAggregateDeviceTapListKey: @[tap_entry],
                @kAudioAggregateDeviceTapAutoStartKey: @NO
            };

            err = AudioHardwareCreateAggregateDevice(
                (__bridge CFDictionaryRef)aggregate_description,
                &aggregate_id
            );
            if (err != noErr || aggregate_id == kAudioObjectUnknown) {
                g_last_probe_status = (int)err;
                fprintf(stderr,
                        "[media_control] aggregate tap device unavailable err=%d device=%u\n",
                        (int)err,
                        aggregate_id);
                AudioHardwareDestroyProcessTap(tap_id);
                return -1;
            }

            char default_name[256];
            AudioDeviceID default_device_id = turbotalk_default_output_device_id();
            turbotalk_object_name(default_device_id, default_name, sizeof(default_name));
            char aggregate_input_format[320];
            char aggregate_output_format[320];
            turbotalk_first_stream_format(aggregate_id,
                                          kAudioObjectPropertyScopeInput,
                                          aggregate_input_format,
                                          sizeof(aggregate_input_format));
            turbotalk_first_stream_format(aggregate_id,
                                          kAudioObjectPropertyScopeOutput,
                                          aggregate_output_format,
                                          sizeof(aggregate_output_format));
            snprintf(g_last_probe_diag,
                     sizeof(g_last_probe_diag),
                     "device_uid=%s device_name=%s tap_id=%u aggregate_id=%u tap_%s agg_input={%s} agg_output={%s}",
                     device_uid.UTF8String ?: "(nil)",
                     default_name,
                     tap_id,
                     aggregate_id,
                     tap_format_diag,
                     aggregate_input_format,
                     aggregate_output_format);
            fprintf(stderr, "[media_control] process tap diag %s\n", g_last_probe_diag);

            AudioProbeContext ctx;
            pthread_mutex_init(&ctx.mutex, NULL);
            pthread_cond_init(&ctx.cond, NULL);
            ctx.sum_squares = 0.0;
            ctx.samples = 0;
            ctx.peak = 0.0f;
            ctx.input_sum_squares = 0.0;
            ctx.input_samples = 0;
            ctx.input_peak = 0.0f;
            ctx.input_buffers = 0;
            ctx.output_sum_squares = 0.0;
            ctx.output_samples = 0;
            ctx.output_peak = 0.0f;
            ctx.output_buffers = 0;
            ctx.format = tap_format;

            err = AudioDeviceCreateIOProcID(
                aggregate_id,
                turbotalk_audio_probe_io_proc,
                &ctx,
                &io_proc_id
            );
            if (err == noErr) {
                err = AudioDeviceStart(aggregate_id, io_proc_id);
            }

            if (err == noErr) {
                struct timespec deadline;
                clock_gettime(CLOCK_REALTIME, &deadline);
                turbotalk_add_millis_to_timespec(&deadline, 220);

                pthread_mutex_lock(&ctx.mutex);
                while (ctx.samples < 2048) {
                    int wait_result = pthread_cond_timedwait(&ctx.cond, &ctx.mutex, &deadline);
                    if (wait_result == ETIMEDOUT) break;
                }

                UInt64 samples = ctx.samples;
                double rms = samples == 0 ? 0.0 : sqrt(ctx.sum_squares / (double)samples);
                float peak = ctx.peak;
                double input_rms =
                    ctx.input_samples == 0
                        ? 0.0
                        : sqrt(ctx.input_sum_squares / (double)ctx.input_samples);
                double output_rms =
                    ctx.output_samples == 0
                        ? 0.0
                        : sqrt(ctx.output_sum_squares / (double)ctx.output_samples);
                result = (samples >= 512 && (rms > 0.00020 || peak > 0.00100)) ? 1 : 0;
                g_last_probe_samples = samples;
                g_last_probe_rms = rms;
                g_last_probe_peak = peak;
                g_last_probe_status = result;
                fprintf(stderr,
                        "[media_control] process tap samples=%llu rms=%.8f peak=%.8f playing=%d input={buffers=%u samples=%llu rms=%.8f peak=%.8f} output={buffers=%u samples=%llu rms=%.8f peak=%.8f}\n",
                        samples,
                        rms,
                        peak,
                        result,
                        ctx.input_buffers,
                        ctx.input_samples,
                        input_rms,
                        ctx.input_peak,
                        ctx.output_buffers,
                        ctx.output_samples,
                        output_rms,
                        ctx.output_peak);
                char setup_diag[sizeof(g_last_probe_diag)];
                snprintf(setup_diag, sizeof(setup_diag), "%s", g_last_probe_diag);
                snprintf(g_last_probe_diag,
                         sizeof(g_last_probe_diag),
                         "%s input={buffers=%u samples=%llu rms=%.8f peak=%.8f} output={buffers=%u samples=%llu rms=%.8f peak=%.8f}",
                         setup_diag,
                         ctx.input_buffers,
                         ctx.input_samples,
                         input_rms,
                         ctx.input_peak,
                         ctx.output_buffers,
                         ctx.output_samples,
                         output_rms,
                         ctx.output_peak);
                pthread_mutex_unlock(&ctx.mutex);

                AudioDeviceStop(aggregate_id, io_proc_id);
            } else {
                g_last_probe_status = (int)err;
                fprintf(stderr, "[media_control] process tap IO unavailable err=%d\n", (int)err);
            }

            if (io_proc_id) AudioDeviceDestroyIOProcID(aggregate_id, io_proc_id);
            pthread_cond_destroy(&ctx.cond);
            pthread_mutex_destroy(&ctx.mutex);
            AudioHardwareDestroyAggregateDevice(aggregate_id);
            AudioHardwareDestroyProcessTap(tap_id);
            return result;
        }
    }

    g_last_probe_status = -3;
    fprintf(stderr, "[media_control] process tap requires macOS 14.2+\n");
    return -1;
}

// Returns:
//   1 = actual output samples crossed the playback threshold
//   0 = tap worked and output was silent
//  -1 = tap unavailable / permission missing / unsupported OS
int audio_is_playing(void) {
    return turbotalk_audio_is_playing_with_process_tap();
}

UInt64 audio_probe_last_samples(void) {
    return g_last_probe_samples;
}

double audio_probe_last_rms(void) {
    return g_last_probe_rms;
}

double audio_probe_last_peak(void) {
    return (double)g_last_probe_peak;
}

int audio_probe_last_status(void) {
    return g_last_probe_status;
}

const char *audio_probe_last_diag(void) {
    return g_last_probe_diag;
}
