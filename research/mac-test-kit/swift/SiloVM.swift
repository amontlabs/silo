// silovm's entry point: picks the command (swift/README.md). Built by lib/build.sh with
// `swiftc -parse-as-library`, so the program starts here.

import Foundation

@main
struct SiloVM {
  static func main() {
    let argv = Array(CommandLine.arguments.dropFirst())
    guard let cmd = argv.first else {
      FileHandle.standardError.write(Data((usageText + "\n").utf8))
      exit(2)
    }
    let rest = Array(argv.dropFirst())
    do {
      switch cmd {
      case "version", "--version":
        printJSON([("silovm", .s(siloVMVersion))])
        exit(0)
      case "help", "--help", "-h":
        printLine(usageText)
        exit(0)
      case "ip":
        exit(try cmdIP(rest))
      case "ctl":
        exit(try cmdCtl(rest))
      default:
        break
      }
      guard #available(macOS 14, *) else {
        throw Fail("silovm needs macOS 14 or later")
      }
      switch cmd {
      case "host-check":
        exit(cmdHostCheck())
      case "ipsw-url":
        cmdIPSWURL()
        dispatchMain()
      case "install":
        try cmdInstall(rest)
        dispatchMain()
      case "clone":
        exit(try cmdClone(rest))
      case "run":
        try cmdRun(rest)
        exit(0)
      default:
        throw Fail("unknown command \(cmd)\n\(usageText)")
      }
    } catch {
      let info = ErrorInfo(error)
      logErr(info.text)
      if cmd == "run" {
        printJSON([("event", .s("stopped")), ("how", .s("error")), ("error", .s(info.text)), ("t", .d(now()))])
      } else {
        printJSON([("ok", .b(false))] + info.fields)
      }
      exit(1)
    }
  }
}
