import Foundation

public struct RunOptions: Equatable {
    public var verbose = false
    public var log = false
    public var debugToggle = false
    public var interfaceName: String?
    public var ouiPath: String?
    public var notify = true
    public var noColor = false
    public var json = false
    public var interval: TimeInterval = 1.0
    public var help = false
    public var noSudo = false

    public init() {}
}

public enum CLIParseResult: Equatable {
    case success(RunOptions)
    case failure(String)
}

public enum CLIParser {
    public static func parse(_ arguments: [String]) -> CLIParseResult {
        var options = RunOptions()
        var index = 0
        while index < arguments.count {
            let arg = arguments[index]
            switch arg {
            case "-v", "--verbose": options.verbose = true
            case "-l", "--log": options.log = true
            case "-d", "--debug": options.debugToggle = true
            case "-h", "--help": options.help = true
            case "--no-notify": options.notify = false
            case "--no-color": options.noColor = true
            case "--no-sudo": options.noSudo = true
            case "--json":
                options.json = true
                options.noColor = true
            case "-i", "--interface":
                index += 1
                guard index < arguments.count else { return .failure("missing value for \(arg)") }
                options.interfaceName = arguments[index]
            case "--oui":
                index += 1
                guard index < arguments.count else { return .failure("missing value for --oui") }
                options.ouiPath = arguments[index]
            case "--interval":
                index += 1
                guard index < arguments.count, let value = Double(arguments[index]), value.isFinite, value > 0, value <= 86_400 else {
                    return .failure("--interval needs a finite number from 0 to 86400")
                }
                options.interval = value
            default:
                if arg.hasPrefix("-") { return .failure("unknown option \(arg)") }
                return .failure("unexpected argument \(arg)")
            }
            index += 1
        }
        return .success(options)
    }

    public static func helpText() -> String {
        """
iairport [options]
  -v, --verbose      print extra airportd lines and IP tags
  -l, --log          write iairport-samples.csv, iairport-roams.csv and bssid_list.txt
  -d, --debug        toggle Wi-Fi debug logging and exit. Root required
  -i, --interface X  Wi-Fi interface. Default is the CoreWLAN interface
      --oui PATH     path to oui.txt
      --no-notify    do not post macOS notifications on roam
      --no-color     plain output
      --no-sudo      do not ask sudo at startup to run log stream as root
      --json         write newline-delimited JSON
      --interval N   sample interval in seconds. Default is 1
  -h, --help         show help
"""
    }
}
