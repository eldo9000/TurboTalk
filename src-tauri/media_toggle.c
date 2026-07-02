// macOS media key toggle + audio-playing detection.
// Compiled into the binary — same process, same permissions.

#import <Cocoa/Cocoa.h>
#import <CoreAudio/CoreAudio.h>
#import <dlfcn.h>

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

// Returns 1 if any media app (Music, Spotify, Chrome, etc.) is actively
// playing content — uses the MediaRemote private framework, same one
// macOS' own Now Playing widget and media keys rely on.
// Falls back to CoreAudio default-output-device query.
int audio_is_playing(void) {
    static dispatch_once_t onceToken;
    static void *mediaRemoteHandle = NULL;
    static void (*getIsPlaying)(dispatch_queue_t, void (^)(BOOL)) = NULL;

    dispatch_once(&onceToken, ^{
        mediaRemoteHandle = dlopen(
            "/System/Library/PrivateFrameworks/"
            "MediaRemote.framework/MediaRemote",
            RTLD_LAZY | RTLD_LOCAL
        );
        if (mediaRemoteHandle) {
            getIsPlaying = dlsym(mediaRemoteHandle,
                "MRMediaRemoteGetNowPlayingApplicationIsPlaying");
        }
    });

    if (getIsPlaying) {
        __block BOOL result = NO;
        __block BOOL done = NO;
        getIsPlaying(dispatch_get_global_queue(DISPATCH_QUEUE_PRIORITY_DEFAULT, 0),
            ^(BOOL playing) {
                result = playing;
                done = YES;
            });
        // Spin-wait up to 1.5 s for the async callback
        for (int i = 0; i < 150 && !done; i++) {
            usleep(10000);
        }
        if (done) return result ? 1 : 0;
    }

    // Fallback: check only the default output device (not all devices —
    // virtual drivers can report spurious "running").
    AudioObjectPropertyAddress defaultAddr = {
        kAudioHardwarePropertyDefaultOutputDevice,
        kAudioObjectPropertyScopeOutput,
        kAudioObjectPropertyElementMain
    };
    AudioDeviceID defaultDevice = kAudioDeviceUnknown;
    UInt32 size = sizeof(defaultDevice);
    OSStatus err = AudioObjectGetPropertyData(
        kAudioObjectSystemObject, &defaultAddr, 0, NULL, &size, &defaultDevice
    );
    if (err != noErr || defaultDevice == kAudioDeviceUnknown) return 0;

    AudioObjectPropertyAddress runningAddr = {
        kAudioDevicePropertyDeviceIsRunningSomewhere,
        kAudioObjectPropertyScopeOutput,
        kAudioObjectPropertyElementMain
    };
    UInt32 isRunning = 0;
    size = sizeof(isRunning);
    err = AudioObjectGetPropertyData(defaultDevice, &runningAddr, 0, NULL, &size, &isRunning);
    return (err == noErr && isRunning) ? 1 : 0;
}
