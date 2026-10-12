import Foundation
import Darwin

struct InterfaceCounters {
    let ibytes: UInt64
    let obytes: UInt64
}

enum CounterError: Error, CustomStringConvertible {
    case getifaddrsFailed(Int32)
    case interfaceNotFound(String)

    var description: String {
        switch self {
        case .getifaddrsFailed(let errnoValue): return "getifaddrs failed errno=\(errnoValue)"
        case .interfaceNotFound(let name): return "interface not found: \(name)"
        }
    }
}

func counters(for interfaceName: String) throws -> InterfaceCounters {
    var first: UnsafeMutablePointer<ifaddrs>?
    if getifaddrs(&first) != 0 {
        throw CounterError.getifaddrsFailed(errno)
    }
    defer { freeifaddrs(first) }

    var cursor = first
    while let current = cursor {
        let ifa = current.pointee
        if let address = ifa.ifa_addr,
           Int32(address.pointee.sa_family) == AF_LINK,
           String(cString: ifa.ifa_name) == interfaceName,
           let dataPointer = ifa.ifa_data {
            let data = dataPointer.assumingMemoryBound(to: if_data.self).pointee
            return InterfaceCounters(ibytes: UInt64(data.ifi_ibytes), obytes: UInt64(data.ifi_obytes))
        }
        cursor = ifa.ifa_next
    }
    throw CounterError.interfaceNotFound(interfaceName)
}

let interfaceName = CommandLine.arguments.dropFirst().first ?? "en0"
let first = try counters(for: interfaceName)
print("sample=1 interface=\(interfaceName) ibytes=\(first.ibytes) obytes=\(first.obytes)")
sleep(1)
let second = try counters(for: interfaceName)
print("sample=2 interface=\(interfaceName) ibytes=\(second.ibytes) obytes=\(second.obytes) deltaI=\(second.ibytes - first.ibytes) deltaO=\(second.obytes - first.obytes)")
