import Foundation
import CoreWLAN
final class D: NSObject, CWEventDelegate {
    func linkQualityDidChangeForWiFiInterface(withName interfaceName: String, rssi: Int, transmitRate: Double) {}
    func ssidDidChangeForWiFiInterface(withName interfaceName: String) {}
}
let d = D()
print(d.responds(to: #selector(CWEventDelegate.linkQualityDidChangeForWiFiInterface(withName:rssi:transmitRate:))))
print(d.responds(to: #selector(CWEventDelegate.ssidDidChangeForWiFiInterface(withName:))))
