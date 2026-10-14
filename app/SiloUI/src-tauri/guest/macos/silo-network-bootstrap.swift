import Darwin
import Foundation
import CryptoKit
import SystemConfiguration

private struct BootstrapConfig: Decodable {
    let version: Int
    let computerId: String
    let expectedMac: String
    let address: String
    let prefixLength: Int
    let router: String
    let dnsServer: String
    let caFingerprint: String
    let probeURL: URL

    var probeHost: String { probeURL.host ?? "" }

    func validate() throws {
        guard version == 1,
              !computerId.isEmpty,
              computerId.utf8.count <= 128,
              isLocallyAdministeredMac(expectedMac),
              prefixLength == 30,
              let guest = ipv4Value(address),
              let gateway = ipv4Value(router),
              dnsServer == router,
              let urlHost = probeURL.host,
              probeURL.scheme?.lowercased() == "https",
              probeURL.user == nil,
              probeURL.password == nil,
              !urlHost.isEmpty else {
            throw BootstrapFailure.invalidConfiguration
        }

        let mask = UInt32.max << 2
        let guestOffset = guest & ~mask
        let gatewayOffset = gateway & ~mask
        guard guest & mask == gateway & mask,
              guestOffset == 2,
              gatewayOffset == 1 else {
            throw BootstrapFailure.invalidConfiguration
        }
        guard caFingerprint.count == 64,
              caFingerprint.allSatisfy({ $0.isHexDigit }) else {
            throw BootstrapFailure.invalidConfiguration
        }
    }
}

private struct Receipt: Encodable {
    let schemaVersion: Int
    let computerId: String
    let bootstrapVersion: Int
    let caFingerprint: String
    let status: String
    let observedAt: Date
    let interface: String?
    let serviceId: String?
    let checks: [String: Bool]
    let failure: String?
}

private struct ConfiguredService {
    let interfaceName: String
    let serviceId: String
    let ipv6Disabled: Bool
}

private enum BootstrapFailure: Error {
    case invalidArguments
    case invalidConfiguration
    case configReadFailed
    case configNotRootOwned
    case requiresRoot
    case preferencesUnavailable
    case preferencesLocked
    case currentSetMissing
    case interfaceNotFound
    case serviceCreateFailed
    case protocolCreateFailed
    case preferencesCommitFailed
    case preferencesApplyFailed
    case activeIPv4NotReady
    case activeRouteNotReady
    case activeDNSNotReady
    case proxyCATrustNotReady
    case resolverFileNotReady
    case scutilResolverNotReady
    case ipv6StillAvailable
    case explicitDNSFailed
    case plainDNSFailed
    case nativeResolverFailed
    case dscacheutilFailed
    case httpsFailed
    case receiptWriteFailed

    var code: String {
        switch self {
        case .invalidArguments: "invalid_arguments"
        case .invalidConfiguration: "invalid_configuration"
        case .configReadFailed: "configuration_read_failed"
        case .configNotRootOwned: "configuration_not_root_owned"
        case .requiresRoot: "root_required"
        case .preferencesUnavailable: "preferences_unavailable"
        case .preferencesLocked: "preferences_lock_failed"
        case .currentSetMissing: "active_network_set_missing"
        case .interfaceNotFound: "expected_mac_not_found"
        case .serviceCreateFailed: "network_service_create_failed"
        case .protocolCreateFailed: "network_protocol_create_failed"
        case .preferencesCommitFailed: "preferences_commit_failed"
        case .preferencesApplyFailed: "preferences_apply_failed"
        case .activeIPv4NotReady: "active_ipv4_not_ready"
        case .activeRouteNotReady: "active_route_not_ready"
        case .activeDNSNotReady: "active_dns_not_ready"
        case .proxyCATrustNotReady: "proxy_ca_trust_not_ready"
        case .resolverFileNotReady: "resolver_file_not_ready"
        case .scutilResolverNotReady: "scutil_resolver_not_ready"
        case .ipv6StillAvailable: "ipv6_still_available"
        case .explicitDNSFailed: "explicit_dns_failed"
        case .plainDNSFailed: "plain_dns_failed"
        case .nativeResolverFailed: "native_resolver_failed"
        case .dscacheutilFailed: "dscacheutil_failed"
        case .httpsFailed: "https_failed"
        case .receiptWriteFailed: "receipt_write_failed"
        }
    }
}

private let receiptPath = "/Library/Application Support/Silo/NetworkBootstrap/receipt.json"

@main
private struct SiloNetworkBootstrap {
    static func main() async {
        do {
            let rawArguments = Array(CommandLine.arguments.dropFirst())
            if rawArguments.count == 2, rawArguments[0] == "--probe-getaddrinfo" {
                exit(nativeAddressLookup(host: rawArguments[1]) ? EXIT_SUCCESS : EXIT_FAILURE)
            }
            let arguments = try parseArguments(rawArguments)
            let config = try loadConfig(at: arguments.configPath, requireRootOwner: !arguments.validateOnly)
            try config.validate()

            if arguments.validateOnly {
                print("configuration valid")
                return
            }

            guard geteuid() == 0 else { throw BootstrapFailure.requiresRoot }
            try clearReceipt()
            try await run(config)
            print("Silo network bootstrap ready")
        } catch let failure as BootstrapFailure {
            fputs("Silo network bootstrap failed: \(failure.code)\n", stderr)
            exit(EXIT_FAILURE)
        } catch {
            fputs("Silo network bootstrap failed: internal_error\n", stderr)
            exit(EXIT_FAILURE)
        }
    }

    private static func parseArguments(_ arguments: [String]) throws -> (configPath: String, validateOnly: Bool) {
        var configPath: String?
        var validateOnly = false
        var index = 0
        while index < arguments.count {
            switch arguments[index] {
            case "--config":
                guard index + 1 < arguments.count, configPath == nil else {
                    throw BootstrapFailure.invalidArguments
                }
                configPath = arguments[index + 1]
                index += 2
            case "--validate-only":
                guard !validateOnly else { throw BootstrapFailure.invalidArguments }
                validateOnly = true
                index += 1
            default:
                throw BootstrapFailure.invalidArguments
            }
        }
        guard let configPath, configPath.hasPrefix("/") else {
            throw BootstrapFailure.invalidArguments
        }
        return (configPath, validateOnly)
    }

    private static func clearReceipt() throws {
        guard FileManager.default.fileExists(atPath: receiptPath) else { return }
        do {
            try FileManager.default.removeItem(atPath: receiptPath)
        } catch {
            throw BootstrapFailure.receiptWriteFailed
        }
    }

    private static func loadConfig(at path: String, requireRootOwner: Bool) throws -> BootstrapConfig {
        guard let data = try? Data(contentsOf: URL(fileURLWithPath: path)),
              let config = try? JSONDecoder().decode(BootstrapConfig.self, from: data) else {
            throw BootstrapFailure.configReadFailed
        }
        if requireRootOwner {
            guard let attributes = try? FileManager.default.attributesOfItem(atPath: path),
                  let owner = attributes[.ownerAccountID] as? NSNumber,
                  owner.intValue == 0,
                  let permissions = attributes[.posixPermissions] as? NSNumber,
                  permissions.intValue & 0o022 == 0 else {
                throw BootstrapFailure.configNotRootOwned
            }
        }
        return config
    }

    private static func run(_ config: BootstrapConfig) async throws {
        var receipt = Receipt(
            schemaVersion: 1,
            computerId: config.computerId,
            bootstrapVersion: config.version,
            caFingerprint: config.caFingerprint.lowercased(),
            status: "failed",
            observedAt: Date(),
            interface: nil,
            serviceId: nil,
            checks: [:],
            failure: nil
        )

        do {
            let service = try await configureWhenInterfaceAppears(config)
            receipt = Receipt(
                schemaVersion: 1,
                computerId: config.computerId,
                bootstrapVersion: config.version,
                caFingerprint: config.caFingerprint.lowercased(),
                status: "failed",
                observedAt: Date(),
                interface: service.interfaceName,
                serviceId: service.serviceId,
                checks: ["ipv6DisabledInPreferences": service.ipv6Disabled],
                failure: nil
            )

            var activeChecks: [String: Bool] = ["ipv6DisabledInPreferences": service.ipv6Disabled]
            var active = false
            for _ in 0..<30 {
                activeChecks = activeNetworkChecks(config, service: service)
                activeChecks["proxyCATrusted"] = trustedProxyCAExists(config.caFingerprint)
                receipt = Receipt(
                    schemaVersion: 1,
                    computerId: config.computerId,
                    bootstrapVersion: config.version,
                    caFingerprint: config.caFingerprint.lowercased(),
                    status: "failed",
                    observedAt: Date(),
                    interface: service.interfaceName,
                    serviceId: service.serviceId,
                    checks: activeChecks,
                    failure: nil
                )
                if activeChecks.values.allSatisfy({ $0 }) {
                    active = true
                    break
                }
                try await Task.sleep(nanoseconds: 1_000_000_000)
            }
            guard active else {
                throw firstFailedCheck(activeChecks)
            }

            var probeChecks: [String: Bool] = [:]
            var probesPassed = false
            for attempt in 0..<3 {
                probeChecks = await networkProbeChecks(config)
                var checks = activeChecks
                checks.merge(probeChecks) { _, newValue in newValue }
                receipt = Receipt(
                    schemaVersion: 1,
                    computerId: config.computerId,
                    bootstrapVersion: config.version,
                    caFingerprint: config.caFingerprint.lowercased(),
                    status: "failed",
                    observedAt: Date(),
                    interface: service.interfaceName,
                    serviceId: service.serviceId,
                    checks: checks,
                    failure: nil
                )
                if probeChecks.values.allSatisfy({ $0 }) {
                    probesPassed = true
                    break
                }
                if attempt < 2 { try await Task.sleep(nanoseconds: 1_000_000_000) }
            }
            guard probesPassed else { throw firstFailedCheck(probeChecks) }

            var allChecks = activeChecks
            allChecks.merge(probeChecks) { _, newValue in newValue }
            receipt = Receipt(
                schemaVersion: 1,
                computerId: config.computerId,
                bootstrapVersion: config.version,
                caFingerprint: config.caFingerprint.lowercased(),
                status: "ready",
                observedAt: Date(),
                interface: service.interfaceName,
                serviceId: service.serviceId,
                checks: allChecks,
                failure: nil
            )
            try writeReceipt(receipt)
        } catch let failure as BootstrapFailure {
            receipt = Receipt(
                schemaVersion: 1,
                computerId: config.computerId,
                bootstrapVersion: config.version,
                caFingerprint: config.caFingerprint.lowercased(),
                status: "failed",
                observedAt: Date(),
                interface: receipt.interface,
                serviceId: receipt.serviceId,
                checks: receipt.checks,
                failure: failure.code
            )
            try? writeReceipt(receipt)
            throw failure
        } catch {
            let failed = Receipt(
                schemaVersion: 1,
                computerId: config.computerId,
                bootstrapVersion: config.version,
                caFingerprint: config.caFingerprint.lowercased(),
                status: "failed",
                observedAt: Date(),
                interface: receipt.interface,
                serviceId: receipt.serviceId,
                checks: receipt.checks,
                failure: "internal_error"
            )
            try? writeReceipt(failed)
            throw BootstrapFailure.receiptWriteFailed
        }
    }

    private static func configureWhenInterfaceAppears(_ config: BootstrapConfig) async throws -> ConfiguredService {
        for attempt in 0..<30 {
            do {
                return try configure(config)
            } catch BootstrapFailure.interfaceNotFound {
                if attempt == 29 { throw BootstrapFailure.interfaceNotFound }
            }
            if attempt < 29 { try await Task.sleep(nanoseconds: 1_000_000_000) }
        }
        throw BootstrapFailure.interfaceNotFound
    }

    private static func configure(_ config: BootstrapConfig) throws -> ConfiguredService {
        guard let preferences = SCPreferencesCreate(nil, "Silo Network Bootstrap" as CFString, nil) else {
            throw BootstrapFailure.preferencesUnavailable
        }
        guard SCPreferencesLock(preferences, true) else { throw BootstrapFailure.preferencesLocked }
        defer { SCPreferencesUnlock(preferences) }

        guard let set = SCNetworkSetCopyCurrent(preferences) else { throw BootstrapFailure.currentSetMissing }
        guard let interfaces = SCNetworkInterfaceCopyAll() as? [SCNetworkInterface] else {
            throw BootstrapFailure.interfaceNotFound
        }
        let matchingInterfaces = interfaces.filter {
            guard let address = SCNetworkInterfaceGetHardwareAddressString($0) else { return false }
            return normalizedMac(address as String) == normalizedMac(config.expectedMac)
        }
        guard matchingInterfaces.count == 1,
              let matchedInterface = matchingInterfaces.first,
              let interfaceName = SCNetworkInterfaceGetBSDName(matchedInterface) as String? else {
            throw BootstrapFailure.interfaceNotFound
        }

        let services = (SCNetworkSetCopyServices(set) as? [SCNetworkService]) ?? []
        var service = services.first(where: { candidate in
            guard let interface = SCNetworkServiceGetInterface(candidate),
                  let bsdName = SCNetworkInterfaceGetBSDName(interface) as String? else { return false }
            return bsdName == interfaceName
        })

        if service == nil {
            guard let created = SCNetworkServiceCreate(preferences, matchedInterface),
                  SCNetworkServiceEstablishDefaultConfiguration(created),
                  SCNetworkSetAddService(set, created) else {
                throw BootstrapFailure.serviceCreateFailed
            }
            service = created
        }
        guard let service else { throw BootstrapFailure.serviceCreateFailed }
        if !SCNetworkServiceGetEnabled(service), !SCNetworkServiceSetEnabled(service, true) {
            throw BootstrapFailure.serviceCreateFailed
        }

        let ipv4 = try protocolFor(service, type: kSCNetworkProtocolTypeIPv4)
        let dns = try protocolFor(service, type: kSCNetworkProtocolTypeDNS)
        let ipv6 = try protocolFor(service, type: kSCNetworkProtocolTypeIPv6)

        let ipv4Configuration: [String: Any] = [
            kSCPropNetIPv4ConfigMethod as String: kSCValNetIPv4ConfigMethodManual as String,
            kSCPropNetIPv4Addresses as String: [config.address],
            kSCPropNetIPv4SubnetMasks as String: ["255.255.255.252"],
            kSCPropNetIPv4Router as String: config.router
        ]
        let dnsConfiguration: [String: Any] = [
            kSCPropNetDNSServerAddresses as String: [config.dnsServer]
        ]
        let ipv6Configuration: [String: Any] = [
            kSCPropNetIPv6ConfigMethod as String: kSCValNetIPv6ConfigMethodLinkLocal as String
        ]

        guard SCNetworkProtocolSetConfiguration(ipv4, ipv4Configuration as CFDictionary),
              SCNetworkProtocolSetEnabled(ipv4, true),
              SCNetworkProtocolSetConfiguration(dns, dnsConfiguration as CFDictionary),
              SCNetworkProtocolSetConfiguration(ipv6, ipv6Configuration as CFDictionary),
              SCNetworkProtocolSetEnabled(ipv6, false) else {
            throw BootstrapFailure.preferencesCommitFailed
        }

        let refreshedServices = (SCNetworkSetCopyServices(set) as? [SCNetworkService]) ?? []
        guard let targetId = SCNetworkServiceGetServiceID(service) as String? else {
            throw BootstrapFailure.serviceCreateFailed
        }
        let order = [targetId] + refreshedServices.compactMap { candidate -> String? in
            guard let id = SCNetworkServiceGetServiceID(candidate) as String?, id != targetId else { return nil }
            return id
        }
        guard SCNetworkSetSetServiceOrder(set, order as CFArray) else {
            throw BootstrapFailure.preferencesCommitFailed
        }
        guard SCPreferencesCommitChanges(preferences) else { throw BootstrapFailure.preferencesCommitFailed }
        guard SCPreferencesApplyChanges(preferences) else { throw BootstrapFailure.preferencesApplyFailed }

        return ConfiguredService(
            interfaceName: interfaceName,
            serviceId: targetId,
            ipv6Disabled: !SCNetworkProtocolGetEnabled(ipv6)
        )
    }

    private static func protocolFor(_ service: SCNetworkService, type: CFString) throws -> SCNetworkProtocol {
        if let existing = SCNetworkServiceCopyProtocol(service, type) { return existing }
        _ = SCNetworkServiceAddProtocolType(service, type)
        guard let created = SCNetworkServiceCopyProtocol(service, type) else {
            throw BootstrapFailure.protocolCreateFailed
        }
        return created
    }

    private static func activeNetworkChecks(_ config: BootstrapConfig, service: ConfiguredService) -> [String: Bool] {
        let store = SCDynamicStoreCreate(nil, "Silo Network Bootstrap Readiness" as CFString, nil, nil)
        guard let store else {
            return [
                "activeIPv4": false,
                "connectedRoute": false,
                "defaultRoute": false,
                "serviceDNS": false,
                "globalDNS": false,
                "resolverFile": false,
                "scutilDNS": false,
                "ipv6Disabled": false
            ]
        }

        let interfaceIPv4 = dynamicDictionary(store, key: "State:/Network/Interface/\(service.interfaceName)/IPv4")
        let activeAddresses = interfaceIPv4?[kSCPropNetIPv4Addresses as String] as? [String] ?? []
        let activeIPv4 = activeAddresses.contains(config.address)

        let serviceDNS = dynamicDictionary(store, key: "State:/Network/Service/\(service.serviceId)/DNS")
        let serviceServers = serviceDNS?[kSCPropNetDNSServerAddresses as String] as? [String] ?? []
        let activeServiceDNS = serviceServers.contains(config.dnsServer)

        let globalDNS = dynamicDictionary(store, key: "State:/Network/Global/DNS")
        let globalServers = globalDNS?[kSCPropNetDNSServerAddresses as String] as? [String] ?? []
        let activeGlobalDNS = globalServers.contains(config.dnsServer)

        let connectedRoute = routeUsesInterface(destination: config.router, interface: service.interfaceName)
        let defaultRoute = routeMatchesDefault(gateway: config.router, interface: service.interfaceName)
        let resolverFile = resolvConfUses(config.dnsServer)
        let scutilDNS = scutilUses(config.dnsServer)
        let ipv6Disabled = service.ipv6Disabled && !hasIPv6DefaultRoute()

        return [
            "activeIPv4": activeIPv4,
            "connectedRoute": connectedRoute,
            "defaultRoute": defaultRoute,
            "serviceDNS": activeServiceDNS,
            "globalDNS": activeGlobalDNS,
            "resolverFile": resolverFile,
            "scutilDNS": scutilDNS,
            "ipv6Disabled": ipv6Disabled
        ]
    }

    /// Compare the expected DER fingerprint with certificates in the System keychain.
    /// The separate HTTPS probe below proves that the guest trust evaluator accepts
    /// the proxy's issued leaf for the configured public host.
    private static func trustedProxyCAExists(_ expectedFingerprint: String) -> Bool {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/security")
        process.arguments = ["find-certificate", "-a", "-p", "/Library/Keychains/System.keychain"]
        let output = Pipe()
        process.standardOutput = output
        process.standardError = FileHandle.nullDevice
        do {
            try process.run()
            let data = output.fileHandleForReading.readDataToEndOfFile()
            process.waitUntilExit()
            guard process.terminationStatus == 0,
                  let text = String(data: data, encoding: .utf8) else { return false }
            let expected = expectedFingerprint.lowercased()
            let blocks = text.components(separatedBy: "-----END CERTIFICATE-----")
            for block in blocks {
                guard let body = block.components(separatedBy: "-----BEGIN CERTIFICATE-----").last else { continue }
                let base64 = body.filter { $0.isLetter || $0.isNumber || $0 == "+" || $0 == "/" || $0 == "=" }
                guard let der = Data(base64Encoded: base64) else { continue }
                let fingerprint = SHA256.hash(data: der).map { String(format: "%02x", $0) }.joined()
                if fingerprint == expected { return true }
            }
        } catch {
            return false
        }
        return false
    }

    private static func networkProbeChecks(_ config: BootstrapConfig) async -> [String: Bool] {
        let explicitDNS = digResolves(host: config.probeHost, server: config.dnsServer)
        let plainDNS = digResolves(host: config.probeHost, server: nil)
        let nativeResolver = nativeGetaddrinfoResolves(host: config.probeHost)
        let cacheResolver = dscacheutilResolves(host: config.probeHost)
        let https = await httpsSucceeds(url: config.probeURL)
        return [
            "explicitDig": explicitDNS,
            "plainDig": plainDNS,
            "getaddrinfo": nativeResolver,
            "dscacheutil": cacheResolver,
            "nativeHTTPS": https
        ]
    }

    private static func routeUsesInterface(destination: String, interface: String) -> Bool {
        let result = runCommand("/sbin/route", ["-n", "get", destination], timeout: 4)
        return result.exitCode == 0 && valueForRouteKey("interface", in: result.output) == interface
    }

    private static func routeMatchesDefault(gateway: String, interface: String) -> Bool {
        let result = runCommand("/sbin/route", ["-n", "get", "default"], timeout: 4)
        return result.exitCode == 0
            && valueForRouteKey("gateway", in: result.output) == gateway
            && valueForRouteKey("interface", in: result.output) == interface
    }

    private static func hasIPv6DefaultRoute() -> Bool {
        runCommand("/sbin/route", ["-n", "get", "-inet6", "default"], timeout: 4).exitCode == 0
    }

    private static func valueForRouteKey(_ key: String, in output: String) -> String? {
        for line in output.split(separator: "\n") {
            let parts = line.split(separator: ":", maxSplits: 1, omittingEmptySubsequences: false)
            guard parts.count == 2, parts[0].trimmingCharacters(in: .whitespaces) == key else { continue }
            return parts[1].trimmingCharacters(in: .whitespaces)
        }
        return nil
    }

    private static func resolvConfUses(_ address: String) -> Bool {
        guard let contents = try? String(contentsOfFile: "/etc/resolv.conf", encoding: .utf8) else { return false }
        return contents.split(whereSeparator: \.isNewline).contains { line in
            let fields = line.split(whereSeparator: \.isWhitespace)
            return fields.count >= 2 && fields[0] == "nameserver" && fields[1] == Substring(address)
        }
    }

    private static func scutilUses(_ address: String) -> Bool {
        let result = runCommand("/usr/sbin/scutil", ["--dns"], timeout: 5)
        guard result.exitCode == 0 else { return false }
        return result.output.split(whereSeparator: \.isNewline).contains { line in
            let fields = line.split(whereSeparator: \.isWhitespace)
            return fields.count >= 2 && fields[0].hasPrefix("nameserver[") && fields.last == Substring(address)
        }
    }

    private static func digResolves(host: String, server: String?) -> Bool {
        var args = ["+time=2", "+tries=1", "+short"]
        if let server { args.append("@\(server)") }
        args += [host, "A"]
        let result = runCommand("/usr/bin/dig", args, timeout: 5)
        return result.exitCode == 0 && !result.output.split(whereSeparator: \.isNewline).isEmpty
    }

    private static func nativeGetaddrinfoResolves(host: String) -> Bool {
        let result = runCommand(CommandLine.arguments[0], ["--probe-getaddrinfo", host], timeout: 5)
        return result.exitCode == 0
    }

    private static func dscacheutilResolves(host: String) -> Bool {
        let result = runCommand("/usr/bin/dscacheutil", ["-q", "host", "-a", "name", host], timeout: 5)
        return result.exitCode == 0 && result.output.contains("ip_address:")
    }

    private static func httpsSucceeds(url: URL) async -> Bool {
        var request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 8)
        request.httpMethod = "GET"
        let configuration = URLSessionConfiguration.ephemeral
        configuration.httpCookieStorage = nil
        configuration.urlCredentialStorage = nil
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        let session = URLSession(configuration: configuration)
        defer { session.invalidateAndCancel() }
        do {
            let (_, response) = try await session.data(for: request)
            guard let http = response as? HTTPURLResponse else { return false }
            return (200..<400).contains(http.statusCode)
        } catch {
            return false
        }
    }

    private static func dynamicDictionary(_ store: SCDynamicStore, key: String) -> [String: Any]? {
        SCDynamicStoreCopyValue(store, key as CFString) as? [String: Any]
    }

    private static func firstFailedCheck(_ checks: [String: Bool]) -> BootstrapFailure {
        if checks["proxyCATrusted"] == false { return .proxyCATrustNotReady }
        if checks["activeIPv4"] == false { return .activeIPv4NotReady }
        if checks["connectedRoute"] == false || checks["defaultRoute"] == false { return .activeRouteNotReady }
        if checks["serviceDNS"] == false || checks["globalDNS"] == false { return .activeDNSNotReady }
        if checks["resolverFile"] == false { return .resolverFileNotReady }
        if checks["scutilDNS"] == false { return .scutilResolverNotReady }
        if checks["ipv6Disabled"] == false { return .ipv6StillAvailable }
        if checks["explicitDig"] == false { return .explicitDNSFailed }
        if checks["plainDig"] == false { return .plainDNSFailed }
        if checks["getaddrinfo"] == false { return .nativeResolverFailed }
        if checks["dscacheutil"] == false { return .dscacheutilFailed }
        return .httpsFailed
    }

    private static func writeReceipt(_ receipt: Receipt) throws {
        let directory = URL(fileURLWithPath: receiptPath).deletingLastPathComponent()
        do {
            try FileManager.default.createDirectory(
                at: directory,
                withIntermediateDirectories: true,
                attributes: [.posixPermissions: 0o755]
            )
            let encoder = JSONEncoder()
            encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
            encoder.dateEncodingStrategy = .iso8601
            let data = try encoder.encode(receipt)
            try data.write(to: URL(fileURLWithPath: receiptPath), options: .atomic)
            try FileManager.default.setAttributes([.posixPermissions: 0o644], ofItemAtPath: receiptPath)
        } catch {
            throw BootstrapFailure.receiptWriteFailed
        }
    }
}

private func normalizedMac(_ input: String) -> String {
    input.lowercased().filter(\.isHexDigit)
}

private func isLocallyAdministeredMac(_ input: String) -> Bool {
    let octets = input.split(separator: ":", omittingEmptySubsequences: false)
    guard octets.count == 6,
          octets.allSatisfy({ $0.count == 2 && UInt8($0, radix: 16) != nil }),
          let first = UInt8(octets[0], radix: 16) else { return false }
    return first & 0x02 != 0 && first & 0x01 == 0
}

private func ipv4Value(_ input: String) -> UInt32? {
    let octets = input.split(separator: ".", omittingEmptySubsequences: false)
    guard octets.count == 4 else { return nil }
    var value: UInt32 = 0
    for octet in octets {
        guard let byte = UInt8(octet) else { return nil }
        value = (value << 8) | UInt32(byte)
    }
    return value
}

private func nativeAddressLookup(host: String) -> Bool {
    var result: UnsafeMutablePointer<addrinfo>?
    let status = getaddrinfo(host, nil, nil, &result)
    if let result { freeaddrinfo(result) }
    return status == 0
}

private struct CommandResult {
    let exitCode: Int32
    let output: String
}

private final class DataBox: @unchecked Sendable {
    private let lock = NSLock()
    private var data = Data()

    func set(_ value: Data) {
        lock.lock()
        data = value
        lock.unlock()
    }

    func get() -> Data {
        lock.lock()
        defer { lock.unlock() }
        return data
    }
}

private func runCommand(_ path: String, _ arguments: [String], timeout: TimeInterval) -> CommandResult {
    let process = Process()
    let outputPipe = Pipe()
    let box = DataBox()
    let outputReadFinished = DispatchSemaphore(value: 0)
    let processFinished = DispatchSemaphore(value: 0)
    process.executableURL = URL(fileURLWithPath: path)
    process.arguments = arguments
    process.standardOutput = outputPipe
    process.standardError = FileHandle.nullDevice
    process.terminationHandler = { _ in processFinished.signal() }
    DispatchQueue.global(qos: .utility).async {
        let output = outputPipe.fileHandleForReading.readDataToEndOfFile()
        box.set(output)
        outputReadFinished.signal()
    }
    do {
        try process.run()
    } catch {
        outputPipe.fileHandleForWriting.closeFile()
        return CommandResult(exitCode: -1, output: "")
    }
    let didFinish = processFinished.wait(timeout: .now() + timeout) == .success
    if !didFinish {
        process.terminate()
        _ = processFinished.wait(timeout: .now() + 2)
    }
    outputPipe.fileHandleForWriting.closeFile()
    _ = outputReadFinished.wait(timeout: .now() + 2)
    return CommandResult(exitCode: didFinish ? process.terminationStatus : -1, output: String(data: box.get(), encoding: .utf8) ?? "")
}
