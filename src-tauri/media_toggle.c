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

// Returns 1 if the default output device has active IO (audio is playing).
// Uses kAudioDevicePropertyDeviceIsRunningSomewhere which works with
// Bluetooth devices (AirPods) and aggregate devices.
int audio_is_playing(void) {
    AudioDeviceID devId = kAudioObjectUnknown;
    UInt32 size = sizeof(devId);
    AudioObjectPropertyAddress defaultAddr = {
        kAudioHardwarePropertyDefaultOutputDevice,
        kAudioObjectPropertyScopeOutput,
        kAudioObjectPropertyElementMain
    };
    AudioObjectGetPropertyData(kAudioObjectSystemObject, &defaultAddr, 0, NULL, &size, &devId);
    if (devId == kAudioObjectUnknown) return 0;

    AudioObjectPropertyAddress addr = {
        kAudioDevicePropertyDeviceIsRunningSomewhere,
        kAudioObjectPropertyScopeOutput,
        kAudioObjectPropertyElementMain
    };
    UInt32 isRunning = 0;
    size = sizeof(isRunning);
    OSStatus err = AudioObjectGetPropertyData(devId, &addr, 0, NULL, &size, &isRunning);
    return (err == noErr && isRunning) ? 1 : 0;
}
