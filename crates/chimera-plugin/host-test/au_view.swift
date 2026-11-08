// Loads CHIMERA's AU view by the recipe Apple documents for hosts, saying how far it gets. REAPER
// showed its generic sliders for this AU, and this tells whether a host doing the usual thing could
// have had the plugin's own view.
import AudioToolbox
import AudioUnit
import AppKit
import CoreAudioKit

var description = AudioComponentDescription(componentType: kAudioUnitType_MusicDevice, componentSubType: 0x566f6963, componentManufacturer: 0x43686d61, componentFlags: 0, componentFlagsMask: 0)
guard let component = AudioComponentFindNext(nil, &description) else { fatalError("no such component") }
var unit: AudioUnit?
print("instantiate:", AudioComponentInstanceNew(component, &unit))
guard let unit else { fatalError("no instance") }
print("initialize:", AudioUnitInitialize(unit))

var size: UInt32 = 0
var writable: DarwinBoolean = false
print("CocoaUI info:", AudioUnitGetPropertyInfo(unit, kAudioUnitProperty_CocoaUI, kAudioUnitScope_Global, 0, &size, &writable), "size", size)
let info = UnsafeMutablePointer<AudioUnitCocoaViewInfo>.allocate(capacity: 1)
print("CocoaUI get:", AudioUnitGetProperty(unit, kAudioUnitProperty_CocoaUI, kAudioUnitScope_Global, 0, info, &size))
let url = info.pointee.mCocoaAUViewBundleLocation.takeUnretainedValue() as URL
let name = info.pointee.mCocoaAUViewClass.takeUnretainedValue() as String
print("bundle:", url.path, "class:", name)

guard let bundle = Bundle(url: url) else { fatalError("no bundle at \(url)") }
print("bundle's executable:", bundle.executablePath ?? "none", "loaded:", bundle.isLoaded, "load:", bundle.load())
guard let found = bundle.classNamed(name) else {
  print("classNamed found nothing; NSClassFromString:", NSClassFromString(name) as Any)
  exit(0)
}
print("class:", found, "conforms to AUCocoaUIBase:", found.conforms(to: AUCocoaUIBase.self))
guard let factory = (found as? NSObject.Type)?.init() as? AUCocoaUIBase else { fatalError("not a view factory") }
let app = NSApplication.shared
_ = app
let view = factory.uiView(forAudioUnit: unit, with: NSSize(width: 1280, height: 800))
print("view:", view as Any, "frame:", view?.frame as Any)
