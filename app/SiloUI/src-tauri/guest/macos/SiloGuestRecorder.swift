import AVFoundation
import CoreMedia
import CoreVideo
import Foundation
import ScreenCaptureKit

final class RecordingDelegate: NSObject, SCRecordingOutputDelegate {
    let finished = DispatchSemaphore(value: 0)
    private(set) var errorText: String?

    func recordingOutputDidFinishRecording(_ recordingOutput: SCRecordingOutput) {
        finished.signal()
    }

    func recordingOutput(_ recordingOutput: SCRecordingOutput, didFailWithError error: Error) {
        errorText = String(describing: error)
        finished.signal()
    }
}

@main
struct SiloGuestRecorder {
    static func main() async {
        let args = Array(CommandLine.arguments.dropFirst())
        guard args.count == 2, let seconds = Double(args[1]), seconds > 0, seconds <= 3600 else {
            fputs("usage: SiloGuestRecorder OUTPUT.mov SECONDS\n", stderr)
            exit(2)
        }

        let outputURL = URL(fileURLWithPath: args[0])
        do {
            try FileManager.default.createDirectory(
                at: outputURL.deletingLastPathComponent(),
                withIntermediateDirectories: true
            )
            try? FileManager.default.removeItem(at: outputURL)
            let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
            guard let display = content.displays.first else { throw RecorderError.noDisplay }
            let filter = SCContentFilter(display: display, excludingApplications: [], exceptingWindows: [])
            let streamConfiguration = SCStreamConfiguration()
            streamConfiguration.width = display.width
            streamConfiguration.height = display.height
            streamConfiguration.minimumFrameInterval = CMTime(value: 1, timescale: 30)
            streamConfiguration.queueDepth = 5
            streamConfiguration.pixelFormat = kCVPixelFormatType_32BGRA
            streamConfiguration.capturesAudio = false

            let outputConfiguration = SCRecordingOutputConfiguration()
            outputConfiguration.outputURL = outputURL
            outputConfiguration.outputFileType = .mov
            outputConfiguration.videoCodecType = .h264
            let delegate = RecordingDelegate()
            let recording = SCRecordingOutput(configuration: outputConfiguration, delegate: delegate)
            let stream = SCStream(filter: filter, configuration: streamConfiguration, delegate: nil)
            try stream.addRecordingOutput(recording)
            try await stream.startCapture()
            print("ScreenCaptureKit started display=\(display.width)x\(display.height) seconds=\(seconds)")
            try await Task.sleep(for: .seconds(seconds))
            try await stream.stopCapture()
            let finishStatus = await Task.detached(priority: .utility) {
                delegate.finished.wait(timeout: .now() + 15)
            }.value
            if finishStatus != .success {
                throw RecorderError.finishTimedOut
            }
            if let errorText = delegate.errorText { throw RecorderError.recordingFailed(errorText) }
            print("ScreenCaptureKit completed file_bytes=\(recording.recordedFileSize) duration_s=\(CMTimeGetSeconds(recording.recordedDuration))")
        } catch {
            fputs("ScreenCaptureKit error: \(error)\n", stderr)
            exit(1)
        }
    }
}

enum RecorderError: Error {
    case noDisplay
    case finishTimedOut
    case recordingFailed(String)
}
