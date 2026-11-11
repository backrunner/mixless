#import <AppKit/AppKit.h>

// Invoked on main before the audio engine or any background data writer starts.
// Escape must never map to the secondary, destructive action in a two-button alert.
int mixless_confirm_replacement(const char *description, const char *action) {
    @autoreleasepool {
        NSApplication *app = [NSApplication sharedApplication];
        NSApplicationActivationPolicy policy = app.activationPolicy;
        [app setActivationPolicy:NSApplicationActivationPolicyRegular];
        NSAlert *alert = [[NSAlert alloc] init];
        alert.alertStyle = NSAlertStyleWarning;
        alert.messageText = @"mixless";
        alert.informativeText = [NSString stringWithUTF8String:description];
        NSButton *quit = [alert addButtonWithTitle:@"Quit"];
        quit.keyEquivalent = @"\r";
        NSButton *proceed = [alert addButtonWithTitle:[NSString stringWithUTF8String:action]];
        proceed.keyEquivalent = @"";
        [alert layout];
        alert.window.title = @"mixless";
        [alert.window center];
        [alert.window makeKeyAndOrderFront:nil];
        id escape = [NSEvent addLocalMonitorForEventsMatchingMask:NSEventMaskKeyDown
            handler:^NSEvent *(NSEvent *event) {
                if (event.keyCode == 53) {
                    [app stopModalWithCode:NSAlertFirstButtonReturn];
                    return nil;
                }
                return event;
            }];
        [app activateIgnoringOtherApps:YES];
        NSModalResponse result = [alert runModal];
        [NSEvent removeMonitor:escape];
        [app setActivationPolicy:policy];
        return result == NSAlertSecondButtonReturn ? 1 : 0;
    }
}

int mixless_reduce_motion(void) {
    return NSWorkspace.sharedWorkspace.accessibilityDisplayShouldReduceMotion ? 1 : 0;
}

// GPUI invokes this on the main thread after creating NSApplication. Embedding
// the image also gives cargo-run/dev.sh builds the same Dock icon as the bundle.
void mixless_set_app_icon(const unsigned char *bytes, unsigned long length) {
    @autoreleasepool {
        NSData *data = [NSData dataWithBytes:bytes length:length];
        NSImage *image = [[NSImage alloc] initWithData:data];
        if (image != nil) {
            [NSApp setApplicationIconImage:image];
        }
    }
}

// A transparent AppKit titlebar can drag before GPUI handles a knob gesture.
// Only the explicitly empty header regions initiate a native window drag.
void mixless_configure_main_window(void) {
    for (NSWindow *window in NSApp.windows) {
        if ([window.title isEqualToString:@"mixless"]) {
            window.movable = NO;
            window.movableByWindowBackground = NO;
        }
    }
}
void mixless_drag_main_window(void) {
    NSEvent *event = NSApp.currentEvent;
    NSWindow *window = event.window;
    if (window && [window.title isEqualToString:@"mixless"]) {
        window.movable = YES;
        [window performWindowDragWithEvent:event];
        window.movable = NO;
    }
}
