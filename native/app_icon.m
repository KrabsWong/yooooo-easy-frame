#import <AppKit/AppKit.h>

void easy_frame_set_dock_icon(const char *path) {
    if (path == NULL) return;
    @autoreleasepool {
        NSString *file = [NSString stringWithUTF8String:path];
        NSImage *image = [[NSImage alloc] initWithContentsOfFile:file];
        if (image != nil) {
            [NSApplication sharedApplication].applicationIconImage = image;
        }
    }
}
