import Foundation
import CoreWLAN
import CoreLocation

// When launched via `open -W`, stdout isn't captured — write to a file instead.
// The output file path is passed as the first argument.
let outputPath: String? = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : nil

final class WiFiScanner: NSObject, CLLocationManagerDelegate {
    private let lm = CLLocationManager()
    private let wifiClient = CWWiFiClient.shared()

    override init() {
        super.init()
        lm.delegate = self
    }

    func start() {
        switch lm.authorizationStatus {
        case .authorizedWhenInUse, .authorizedAlways:
            doScan()
        case .notDetermined:
            lm.requestWhenInUseAuthorization()
        case .denied, .restricted:
            emit(connected: nil, networks: [])
            exit(0)
        @unknown default:
            emit(connected: nil, networks: [])
            exit(0)
        }
    }

    func locationManagerDidChangeAuthorization(_ manager: CLLocationManager) {
        switch manager.authorizationStatus {
        case .authorizedWhenInUse, .authorizedAlways:
            doScan()
        case .denied, .restricted:
            emit(connected: nil, networks: [])
            exit(0)
        case .notDetermined:
            break
        @unknown default:
            exit(0)
        }
    }

    private func doScan() {
        guard let iface = wifiClient.interface() else {
            emit(connected: nil, networks: [])
            exit(0)
        }

        // CWInterface.ssid is a method on some SDK versions — call with ()
        let rawConnected: String? = iface.ssid()
        var ssids = Set<String>()

        for attempt in 1...3 {
            if let networks = try? iface.scanForNetworks(withSSID: nil) {
                for n in networks {
                    if let s = n.ssid, !s.isEmpty, !s.lowercased().contains("redacted") {
                        ssids.insert(s)
                    }
                }
                if !ssids.isEmpty { break }
            }
            if attempt < 3 { Thread.sleep(forTimeInterval: 1.0) }
        }

        let connectedSSID: String? = rawConnected.flatMap {
            $0.isEmpty || $0.lowercased().contains("redacted") ? nil : $0
        }
        if let c = connectedSSID { ssids.insert(c) }

        emit(connected: connectedSSID, networks: ssids.sorted())
        exit(0)
    }

    private func emit(connected: String?, networks: [String]) {
        var obj: [String: Any] = ["networks": networks]
        obj["connected"] = connected.map { $0 as Any } ?? NSNull()
        guard
            let data = try? JSONSerialization.data(withJSONObject: obj, options: [.sortedKeys]),
            let s = String(data: data, encoding: .utf8)
        else { return }

        if let path = outputPath {
            try? s.write(toFile: path, atomically: true, encoding: .utf8)
        } else {
            print(s)
        }
    }
}

// 30-second global timeout guard
DispatchQueue.global().asyncAfter(deadline: .now() + 30) {
    let msg = "{\"error\":\"timeout\",\"connected\":null,\"networks\":[]}"
    if let path = outputPath {
        try? msg.write(toFile: path, atomically: true, encoding: .utf8)
    } else {
        fputs(msg + "\n", stderr)
    }
    exit(1)
}

let scanner = WiFiScanner()
scanner.start()
RunLoop.main.run()
