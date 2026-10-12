import Foundation
import SystemConfiguration

let key = "State:/Network/Interface/en0/AirPort" as CFString
guard let value = SCDynamicStoreCopyValue(nil, key) as? [String: Any] else {
    print("no dynamic store dictionary")
    exit(1)
}
print("keys=\(value.keys.sorted())")
if let data = value["CachedScanRecord"] as? Data {
    print("CachedScanRecord bytes=\(data.count)")
    do {
        let classes: [AnyClass] = [NSDictionary.self, NSString.self, NSNumber.self, NSData.self, NSArray.self, NSDate.self]
        let obj = try NSKeyedUnarchiver.unarchivedObject(ofClasses: classes, from: data)
        print("unarchived type=\(type(of: obj as Any))")
        if let d = obj as? [String: Any] {
            for k in d.keys.sorted() { print("\(k)=\(d[k]!)") }
        } else if let d = obj as? NSDictionary {
            print("NSDictionary keys=\(d.allKeys)")
        }
    } catch {
        print("unarchive error=\(error)")
        var fmt = PropertyListSerialization.PropertyListFormat.binary
        let plist = try PropertyListSerialization.propertyList(from: data, options: [], format: &fmt)
        print("plist type=\(type(of: plist)) format=\(fmt)")
        if let pd = plist as? [String: Any] {
            print("plist keys=\(pd.keys.sorted())")
            if let top = pd["$top"] { print("top=\(top) type=\(type(of: top)) mirror=\(Mirror(reflecting: top))") }
            if let objs = pd["$objects"] as? [Any] {
                print("objects count=\(objs.count)")
                for i in 0..<min(10, objs.count) { print("obj[\(i)] type=\(type(of: objs[i])) value=\(objs[i]) mirror=\(Mirror(reflecting: objs[i]))") }
            }
        }
    }
} else {
    print("no CachedScanRecord; value types:")
    for (k,v) in value { print("\(k): \(type(of:v))") }
}
