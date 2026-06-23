// macOS media key toggle + audio-playing detection.
// Compiled into the binary — same process, same permissions.

#import <Cocoa/Cocoa.h>
#import <CoreAudio/CoreAudio.h>

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

// Returns 1 if ANY output device has active IO (audio is playing).
// Iterates all devices to handle Bluetooth/aggregate device quirks.
int audio_is_playing(void) {
    // Get list of all audio device IDs
    AudioObjectPropertyAddress devListAddr = {
        kAudioHardwarePropertyDevices,
        kAudioObjectPropertyScopeOutput,
        kAudioObjectPropertyElementMain
    };
    UInt32 dataSize = 0;
    AudioObjectGetPropertyDataSize(kAudioObjectSystemObject, &devListAddr, 0, NULL, &dataSize);
    UInt32 deviceCount = dataSize / sizeof(AudioDeviceID);
    if (deviceCount == 0) return 0;

    AudioDeviceID *devices = malloc(dataSize);
    if (!devices) return 0;
    AudioObjectGetPropertyData(kAudioObjectSystemObject, &devListAddr, 0, NULL, &dataSize, devices);

    AudioObjectPropertyAddress runningAddr = {
        kAudioDevicePropertyDeviceIsRunningSomewhere,
        kAudioObjectPropertyScopeOutput,
        kAudioObjectPropertyElementMain
    };

    int found = 0;
    for (UInt32 i = 0; i < deviceCount; i++) {
        UInt32 isRunning = 0;
        UInt32 size = sizeof(isRunning);
        OSStatus err = AudioObjectGetPropertyData(devices[i], &runningAddr, 0, NULL, &size, &isRunning);
        if (err == noErr && isRunning) {
            found = 1;
            break;
        }
    }
    free(devices);
    return found;
}
