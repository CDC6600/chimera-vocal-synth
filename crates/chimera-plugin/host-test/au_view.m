// Loads CHIMERA's AU view by the recipe Apple documents for hosts, saying how far it gets, with no
// other format in the process. REAPER showed its generic sliders for this AU.
#import <AppKit/AppKit.h>
#import <AudioToolbox/AudioToolbox.h>
#import <AudioUnit/AUCocoaUIView.h>

int main(void) {
  @autoreleasepool {
    [NSApplication sharedApplication];
    AudioComponentDescription description = {kAudioUnitType_MusicDevice, 'Voic', 'Chma', 0, 0};
    AudioComponent component = AudioComponentFindNext(NULL, &description);
    if (!component) { puts("no such component"); return 1; }
    AudioUnit unit = NULL;
    printf("instantiate: %d\n", (int)AudioComponentInstanceNew(component, &unit));
    printf("initialize: %d\n", (int)AudioUnitInitialize(unit));

    UInt32 size = 0;
    Boolean writable = false;
    printf("view info: %d, size %u\n", (int)AudioUnitGetPropertyInfo(unit, kAudioUnitProperty_CocoaUI, kAudioUnitScope_Global, 0, &size, &writable), size);
    AudioUnitCocoaViewInfo info;
    printf("view get: %d\n", (int)AudioUnitGetProperty(unit, kAudioUnitProperty_CocoaUI, kAudioUnitScope_Global, 0, &info, &size));
    NSURL *url = (__bridge NSURL *)info.mCocoaAUViewBundleLocation;
    NSString *name = (__bridge NSString *)info.mCocoaAUViewClass[0];
    printf("bundle %s, class %s\n", url.path.UTF8String, name.UTF8String);

    NSBundle *bundle = [NSBundle bundleWithURL:url];
    printf("bundle found: %d, executable %s\n", bundle != nil, bundle.executablePath.UTF8String);
    printf("load: %d\n", [bundle load]);
    Class found = [bundle classNamed:name];
    printf("classNamed: %s, NSClassFromString: %s\n", found ? "found" : "NOTHING", NSClassFromString(name) ? "found" : "NOTHING");
    if (!found) return 1;
    printf("conforms: %d\n", [found conformsToProtocol:@protocol(AUCocoaUIBase)]);
    id<AUCocoaUIBase> factory = [[found alloc] init];
    NSView *view = [factory uiViewForAudioUnit:unit withSize:NSMakeSize(1280, 800)];
    printf("view: %s, %.0f x %.0f\n", view ? NSStringFromClass([view class]).UTF8String : "NOTHING", view.frame.size.width, view.frame.size.height);
    return view ? 0 : 1;
  }
}
