// macOS media key toggle — compiled into the binary, same process = same
// Accessibility permissions as the main app.

#import <Cocoa/Cocoa.h>

void media_toggle_play_pause(void) {
    @autoreleasepool {
        int keyCode = 16; // NX_KEYTYPE_PLAY

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
        if (cgDown) {
            CGEventPost(kCGSessionEventTap, cgDown);
        }

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
        if (cgUp) {
            CGEventPost(kCGSessionEventTap, cgUp);
        }
    }
}
