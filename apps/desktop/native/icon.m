#import <AppKit/AppKit.h>

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
