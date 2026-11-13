// rune-listen: the ears of Rune.
//
// A small program the app starts when you ask it to listen. It hears the
// microphone ("me") and, for a meeting, the sound this Mac is playing
// ("others"), turns the speech into text on this Mac (Apple's on-device
// speech recognition; no audio is kept or sent anywhere), and prints the
// text as JSON lines for the app to read:
//
//   {"t":"status","state":"listening","on_device":true,"others":true}
//   {"t":"partial","who":"me","text":"so the plan is"}
//   {"t":"final","who":"me","text":"So the plan is to ship on Friday.","at":12.4}
//   {"t":"error","what":"microphone","message":"…"}      (then it stops)
//   {"t":"note","what":"system-audio","message":"…"}     (it carries on)
//   {"t":"done"}
//
// It stops when the app writes "stop" to it, closes its input, or ends it.
//
//   rune-listen --mic       the microphone only (the voice box)
//   rune-listen --meeting   the microphone and the Mac's sound (a meeting)
//   rune-listen --system    the Mac's sound only, as "others" (started by
//                        --meeting itself, as a second copy of this program)
//   rune-listen --system --from-app P   only the sound of the app with process id P
//                        and the helpers it plays through (a video in Rune's player)
//   rune-listen --check     say what is allowed, ask for nothing, hear nothing
//   rune-listen --file F    as if the microphone heard the sound file F (a test
//                        of the recognition that needs no microphone)
//   --debug              with any of them: also say how much sound arrives
//                        and what the recognition complains about
//
// macOS asks the person to allow each of: the microphone, speech recognition
// and (for a meeting) recording the Mac's audio. The app's Info.plist says why.
//
// Built by scripts/bundle.sh into the app, beside the main program.

import AVFoundation
import AudioToolbox
import CoreAudio
import Foundation
import Speech

// MARK: - Talking to the app

let outLock = NSLock()

func emit(_ object: [String: Any]) {
    guard let data = try? JSONSerialization.data(withJSONObject: object), var line = String(data: data, encoding: .utf8) else { return }
    line += "\n"
    outLock.lock()
    FileHandle.standardOutput.write(line.data(using: .utf8) ?? Data())
    outLock.unlock()
}

/// `--debug`: lines the app ignores, for finding out why nothing is heard.
let debugging = CommandLine.arguments.contains("--debug")

func debug(_ object: [String: Any]) {
    guard debugging else { return }
    emit(object.merging(["t": "debug"]) { _, new in new })
}

func fail(_ what: String, _ message: String) -> Never {
    emit(["t": "error", "what": what, "message": message])
    exit(2)
}

// MARK: - Speech into text

/// Turns one stream of sound into text. Speech recognition works on a
/// stretch of sound at a time, so a stretch ends when the speaker has been
/// quiet for a moment (its text is then final) and the next one begins.
final class Transcriber {
    let who: String
    let recognizer: SFSpeechRecognizer
    let started = Date()
    private let lock = NSLock()
    private var request: SFSpeechAudioBufferRecognitionRequest?
    private var task: SFSpeechRecognitionTask?
    private var text = ""
    /// How many of the first words of `text` are final already (said to the
    /// app at a pause, while the stretch goes on).
    private var done = 0
    private var changed = Date()
    private var stopping = false
    private var began = Date()
    private var timer: DispatchSourceTimer?
    private let queue = DispatchQueue(label: "rune-listen.transcriber")
    // For --debug: how much sound came in since the last report.
    private var buffers = 0
    private var peak: Float = 0
    private var ticks = 0

    init?(who: String, locale: Locale) {
        guard let recognizer = SFSpeechRecognizer(locale: locale) ?? SFSpeechRecognizer() else { return nil }
        self.who = who
        self.recognizer = recognizer
    }

    var onDevice: Bool { recognizer.supportsOnDeviceRecognition }

    func begin() {
        queue.async { self.beginStretch() }
        let timer = DispatchSource.makeTimerSource(queue: queue)
        timer.schedule(deadline: .now() + 0.5, repeating: 0.5)
        timer.setEventHandler { [weak self] in self?.endStretchIfQuiet() }
        timer.resume()
        self.timer = timer
    }

    private func beginStretch() {
        let request = SFSpeechAudioBufferRecognitionRequest()
        request.shouldReportPartialResults = true
        request.addsPunctuation = true
        request.taskHint = .dictation
        if recognizer.supportsOnDeviceRecognition {
            request.requiresOnDeviceRecognition = true
        }
        text = ""
        done = 0
        changed = Date()
        began = Date()
        lock.lock()
        self.request = request
        lock.unlock()
        task = recognizer.recognitionTask(with: request) { [weak self] result, error in
            guard let self else { return }
            self.queue.async {
                // Only the stretch in progress may speak.
                self.lock.lock()
                let current = self.request === request
                self.lock.unlock()
                guard current else { return }
                if let result {
                    let heard = result.bestTranscription.formattedString
                    if result.isFinal && self.isEndOfText(heard) {
                        // The last answer of a stretch can hold only the end
                        // of what was said. All of it stays.
                    } else if heard != self.text {
                        // After a short pause the speech engine of the Mac
                        // starts its text again, in the same stretch. What
                        // was said before the pause is then final: without
                        // this it went from the screen and was lost.
                        if self.startedAgain(heard) {
                            self.say(final: self.rest(self.text))
                            self.done = 0
                        }
                        self.text = heard
                        self.done = min(self.done, self.pieces(heard).count)
                        self.changed = Date()
                        let more = self.rest(heard)
                        if !result.isFinal && !more.isEmpty {
                            emit(["t": "partial", "who": self.who, "text": more])
                        }
                    }
                }
                if let error = error as NSError? {
                    debug(["who": self.who, "error": "\(error.domain) \(error.code): \(error.localizedDescription)"])
                }
                if error != nil || (result?.isFinal ?? false) {
                    self.closeStretch()
                }
            }
        }
    }

    /// A text as its words, as they are written.
    private func pieces(_ text: String) -> [Substring] {
        text.split(whereSeparator: { $0 == " " || $0 == "\n" })
    }

    /// The words of a text that are not final yet.
    private func rest(_ text: String) -> String {
        pieces(text).dropFirst(done).joined(separator: " ")
    }

    /// The words of a text, in small letters, with no signs at their ends.
    private func words(_ text: String) -> [String] {
        text.split(whereSeparator: { $0 == " " || $0 == "\n" }).map { $0.lowercased().trimmingCharacters(in: .punctuationCharacters) }.filter { !$0.isEmpty }
    }

    /// Is `heard` a new start of the text, and not a correction of it? A
    /// correction comes while the person speaks, and it keeps the start of
    /// the text or most of its length. A new start is short, and it comes
    /// after a pause or begins with another word.
    private func startedAgain(_ heard: String) -> Bool {
        let old = words(text)
        let new = words(heard)
        guard !old.isEmpty else { return false }
        if new.isEmpty { return true }
        let paused = Date().timeIntervalSince(changed) > 0.7
        let short = new.count * 2 <= old.count || (paused && new.count <= 2 && new.count <= old.count)
        return short && (paused || new.first != old.first)
    }

    /// Is `heard` only the last words of the text that is there?
    private func isEndOfText(_ heard: String) -> Bool {
        let old = words(text)
        let new = words(heard)
        return !new.isEmpty && new.count < old.count && Array(old.suffix(new.count)) == new
    }

    /// These words are final.
    private func say(final text: String) {
        let said = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if !said.isEmpty {
            emit(["t": "final", "who": who, "text": said, "at": (Date().timeIntervalSince(started) * 10).rounded() / 10])
        }
    }

    /// A pause: what was said up to here is final, and the stretch goes on.
    /// (A new stretch at each pause lost the first words after the pause:
    /// they went to the stretch that was at its end.) Only a long stretch
    /// ends, at a longer silence, so that no stretch is without end.
    private func endStretchIfQuiet() {
        if debugging {
            ticks += 1
            if ticks % 2 == 0 {
                lock.lock()
                let (count, loudest) = (buffers, peak)
                buffers = 0
                peak = 0
                lock.unlock()
                debug(["who": who, "buffers": count, "peak": (Double(loudest) * 1000).rounded() / 1000, "task": task != nil])
            }
        }
        let quiet = Date().timeIntervalSince(changed)
        guard !text.isEmpty, quiet > 1.6 else { return }
        let more = rest(text)
        if !more.isEmpty {
            say(final: more)
            done = pieces(text).count
        }
        guard Date().timeIntervalSince(began) > 50, quiet > 3 else { return }
        lock.lock()
        let request = self.request
        lock.unlock()
        request?.endAudio()
        // If no last answer comes, close it ourselves.
        if quiet > 6 {
            closeStretch()
        }
    }

    private func closeStretch() {
        say(final: rest(text))
        text = ""
        done = 0
        task?.cancel()
        task = nil
        lock.lock()
        request = nil
        lock.unlock()
        guard !stopping else { return }
        if Date().timeIntervalSince(began) >= 1 {
            beginStretch()
        } else {
            // It failed as soon as it began. Wait a moment, so that a fault
            // that repeats cannot make this spin.
            queue.asyncAfter(deadline: .now() + 0.5) {
                self.lock.lock()
                let idle = self.request == nil
                self.lock.unlock()
                if idle && !self.stopping {
                    self.beginStretch()
                }
            }
        }
    }

    /// Sound for the stretch in progress (called from the audio threads).
    func append(_ buffer: AVAudioPCMBuffer) {
        lock.lock()
        let request = self.request
        if debugging {
            buffers += 1
            if let samples = buffer.floatChannelData?[0] {
                for i in 0..<Int(buffer.frameLength) {
                    peak = max(peak, abs(samples[i]))
                }
            }
        }
        lock.unlock()
        request?.append(buffer)
    }

    /// Finish: the last words become final. Calls `then` when done.
    func end(_ then: @escaping () -> Void) {
        queue.async {
            self.stopping = true
            self.timer?.cancel()
            self.lock.lock()
            let request = self.request
            self.lock.unlock()
            request?.endAudio()
            self.queue.asyncAfter(deadline: .now() + 1.2) {
                self.closeStretch()
                then()
            }
        }
    }
}

// MARK: - The microphone

final class Microphone {
    private let engine = AVAudioEngine()

    func start(_ sink: @escaping (AVAudioPCMBuffer) -> Void) throws {
        let input = engine.inputNode
        let format = input.outputFormat(forBus: 0)
        guard format.sampleRate > 0, format.channelCount > 0 else {
            throw NSError(domain: "rune-listen", code: 1, userInfo: [NSLocalizedDescriptionKey: "There is no microphone to listen to."])
        }
        input.installTap(onBus: 0, bufferSize: 4096, format: format) { buffer, _ in sink(buffer) }
        engine.prepare()
        try engine.start()
    }

    func stop() {
        engine.inputNode.removeTap(onBus: 0)
        engine.stop()
    }
}

// MARK: - The sound this Mac plays (the other people in a meeting)

/// A "tap" on everything the Mac plays, read through a private device made
/// for it (Core Audio taps, macOS 14.2 and later). macOS asks the person to
/// allow audio recording the first time; if they say no, this hears silence.
final class SystemSound {
    private var tap = AudioObjectID(kAudioObjectUnknown)
    private var device = AudioObjectID(kAudioObjectUnknown)
    private var proc: AudioDeviceIOProcID?
    private let queue = DispatchQueue(label: "rune-listen.system-sound")

    private func check(_ status: OSStatus, _ what: String) throws {
        if status != noErr {
            throw NSError(domain: "rune-listen", code: Int(status), userInfo: [NSLocalizedDescriptionKey: "\(what) (error \(status))."])
        }
    }

    private func outputDeviceUID() throws -> String {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyDefaultSystemOutputDevice,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain)
        var output = AudioObjectID(kAudioObjectUnknown)
        var size = UInt32(MemoryLayout<AudioObjectID>.size)
        try check(AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size, &output), "Couldn't find the Mac's sound output")
        address.mSelector = kAudioDevicePropertyDeviceUID
        var uid: CFString = "" as CFString
        size = UInt32(MemoryLayout<CFString>.size)
        try withUnsafeMutablePointer(to: &uid) { pointer in
            try check(AudioObjectGetPropertyData(output, &address, 0, nil, &size, pointer), "Couldn't read the Mac's sound output")
        }
        return uid as String
    }

    /// The programs that play sound for the app `app` (its own program, and
    /// the helpers it plays through: a web view plays through WebKit's own
    /// programs), as Core Audio knows them now. macOS says which app each
    /// program works for.
    static func soundOf(app: pid_t) -> [AudioObjectID] {
        typealias Responsible = @convention(c) (pid_t) -> pid_t
        let responsible: Responsible? = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "responsibility_get_pid_responsible_for_pid").map { unsafeBitCast($0, to: Responsible.self) }
        var address = AudioObjectPropertyAddress(mSelector: kAudioHardwarePropertyProcessObjectList, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
        var size: UInt32 = 0
        guard AudioObjectGetPropertyDataSize(AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size) == noErr, size > 0 else { return [] }
        var objects = [AudioObjectID](repeating: 0, count: Int(size) / MemoryLayout<AudioObjectID>.size)
        guard AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size, &objects) == noErr else { return [] }
        let me = getpid()
        return objects.filter { object in
            var pid: pid_t = 0
            var where_ = AudioObjectPropertyAddress(mSelector: kAudioProcessPropertyPID, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
            var pidSize = UInt32(MemoryLayout<pid_t>.size)
            guard AudioObjectGetPropertyData(object, &where_, 0, nil, &pidSize, &pid) == noErr, pid > 0, pid != me else { return false }
            return pid == app || responsible?(pid) == app
        }
    }

    /// With `app`, only the sound of that app (a video in Rune's player, not
    /// a call in another program); without it, all that the Mac plays.
    func start(app: pid_t? = nil, _ sink: @escaping (AVAudioPCMBuffer) -> Void) throws {
        let description: CATapDescription
        if let app {
            let own = SystemSound.soundOf(app: app)
            if own.isEmpty {
                throw NSError(domain: "rune-listen", code: 3, userInfo: [NSLocalizedDescriptionKey: "Rune's player has no sound yet. Play the video, then try again."])
            }
            description = CATapDescription(stereoMixdownOfProcesses: own)
        } else {
            description = CATapDescription(stereoGlobalTapButExcludeProcesses: [])
        }
        description.uuid = UUID()
        description.name = "Rune listener"
        description.isPrivate = true
        description.muteBehavior = .unmuted
        try check(AudioHardwareCreateProcessTap(description, &tap), "Couldn't start hearing the Mac's sound")

        let output = try outputDeviceUID()
        let aggregate: [String: Any] = [
            kAudioAggregateDeviceNameKey: "Rune listener",
            kAudioAggregateDeviceUIDKey: UUID().uuidString,
            kAudioAggregateDeviceMainSubDeviceKey: output,
            kAudioAggregateDeviceIsPrivateKey: true,
            kAudioAggregateDeviceIsStackedKey: false,
            kAudioAggregateDeviceTapAutoStartKey: true,
            kAudioAggregateDeviceSubDeviceListKey: [[kAudioSubDeviceUIDKey: output]],
            kAudioAggregateDeviceTapListKey: [[kAudioSubTapDriftCompensationKey: true, kAudioSubTapUIDKey: description.uuid.uuidString]],
        ]
        try check(AudioHardwareCreateAggregateDevice(aggregate as CFDictionary, &device), "Couldn't set up hearing the Mac's sound")

        var address = AudioObjectPropertyAddress(
            mSelector: kAudioTapPropertyFormat,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain)
        var stream = AudioStreamBasicDescription()
        var size = UInt32(MemoryLayout<AudioStreamBasicDescription>.size)
        try check(AudioObjectGetPropertyData(tap, &address, 0, nil, &size, &stream), "Couldn't read the form of the Mac's sound")
        guard let format = AVAudioFormat(streamDescription: &stream) else {
            throw NSError(domain: "rune-listen", code: 2, userInfo: [NSLocalizedDescriptionKey: "The Mac's sound is in a form this can't read."])
        }

        try check(
            AudioDeviceCreateIOProcIDWithBlock(&proc, device, queue) { _, input, _, _, _ in
                // The sound is only ours for the length of this call: copy it.
                guard let borrowed = AVAudioPCMBuffer(pcmFormat: format, bufferListNoCopy: input, deallocator: nil),
                    let copy = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: borrowed.frameLength)
                else { return }
                copy.frameLength = borrowed.frameLength
                let from = UnsafeMutableAudioBufferListPointer(UnsafeMutablePointer(mutating: borrowed.audioBufferList))
                let to = UnsafeMutableAudioBufferListPointer(copy.mutableAudioBufferList)
                for (source, target) in zip(from, to) {
                    if let s = source.mData, let t = target.mData {
                        memcpy(t, s, Int(min(source.mDataByteSize, target.mDataByteSize)))
                    }
                }
                sink(copy)
            }, "Couldn't read the Mac's sound")
        try check(AudioDeviceStart(device, proc), "Couldn't start reading the Mac's sound")
    }

    func stop() {
        if device != kAudioObjectUnknown {
            if let proc {
                AudioDeviceStop(device, proc)
                AudioDeviceDestroyIOProcID(device, proc)
            }
            AudioHardwareDestroyAggregateDevice(device)
            device = AudioObjectID(kAudioObjectUnknown)
        }
        if tap != kAudioObjectUnknown {
            AudioHardwareDestroyProcessTap(tap)
            tap = AudioObjectID(kAudioObjectUnknown)
        }
    }
}

// MARK: - The other people, heard by a second copy of this program

/// Where this program is.
func ownPath() -> String {
    var size = UInt32(4096)
    var buffer = [CChar](repeating: 0, count: Int(size))
    if _NSGetExecutablePath(&buffer, &size) == 0 {
        return String(cString: buffer)
    }
    return CommandLine.arguments[0]
}

/// macOS runs one speech recognition at a time in a program: two in one
/// program stop each other, and neither writes a word. So in a meeting the
/// sound the Mac plays is heard by a second copy of this program
/// (`--system`), and what it writes is passed on from here.
final class Others {
    private let process = Process()
    private let input = Pipe()
    private let output = Pipe()
    private let lock = NSLock()
    private var answered = false
    private var problem: String?
    private let ready = DispatchSemaphore(value: 0)
    private let finished = DispatchSemaphore(value: 0)

    /// Start it. Returns nil when it is listening, or says why it is not.
    func start(_ extra: [String]) -> String? {
        process.executableURL = URL(fileURLWithPath: ownPath())
        process.arguments = ["--system"] + extra
        process.standardInput = input
        process.standardOutput = output
        process.standardError = FileHandle.nullDevice
        do {
            try process.run()
        } catch {
            return error.localizedDescription
        }
        DispatchQueue.global().async { self.read() }
        if ready.wait(timeout: .now() + 8) == .timedOut {
            process.terminate()
            return "It did not start."
        }
        return problem
    }

    private func settle(_ why: String?) {
        lock.lock()
        let first = !answered
        if first {
            answered = true
            problem = why
        }
        lock.unlock()
        if first {
            ready.signal()
        }
    }

    private func read() {
        let handle = output.fileHandleForReading
        var rest = Data()
        while true {
            let data = handle.availableData
            if data.isEmpty { break }
            rest.append(data)
            while let end = rest.firstIndex(of: 10) {
                let line = rest.subdata(in: rest.startIndex..<end)
                rest.removeSubrange(rest.startIndex...end)
                take(line)
            }
        }
        settle("It stopped.")
        finished.signal()
    }

    private func take(_ line: Data) {
        guard let object = (try? JSONSerialization.jsonObject(with: line)) as? [String: Any], let kind = object["t"] as? String else { return }
        switch kind {
        case "status": settle(nil)
        case "error": settle(object["message"] as? String ?? "It could not start.")
        case "partial", "final", "debug": emit(object)
        default: break // its "done": ours follows
        }
    }

    /// Ask it to finish, and wait a moment for its last words.
    func stop() {
        try? input.fileHandleForWriting.write(contentsOf: Data("stop\n".utf8))
        try? input.fileHandleForWriting.close()
        if finished.wait(timeout: .now() + 4) == .timedOut {
            process.terminate()
        }
    }
}

// MARK: - What is allowed

func speechWord(_ status: SFSpeechRecognizerAuthorizationStatus) -> String {
    switch status {
    case .authorized: return "allowed"
    case .denied: return "denied"
    case .restricted: return "restricted"
    default: return "not asked"
    }
}

func microphoneWord(_ status: AVAuthorizationStatus) -> String {
    switch status {
    case .authorized: return "allowed"
    case .denied: return "denied"
    case .restricted: return "restricted"
    default: return "not asked"
    }
}

/// Wait for the answer to a question macOS puts to the person.
func ask(_ question: (@escaping (Bool) -> Void) -> Void) -> Bool {
    let done = DispatchSemaphore(value: 0)
    var answer = false
    question { yes in
        answer = yes
        done.signal()
    }
    done.wait()
    return answer
}

// MARK: - Main

let arguments = CommandLine.arguments
var locale = Locale.current
if let i = arguments.firstIndex(of: "--locale"), i + 1 < arguments.count {
    locale = Locale(identifier: arguments[i + 1])
}

if arguments.contains("--check") {
    let recognizer = SFSpeechRecognizer(locale: locale) ?? SFSpeechRecognizer()
    emit([
        "t": "check",
        "microphone": microphoneWord(AVCaptureDevice.authorizationStatus(for: .audio)),
        "speech": speechWord(SFSpeechRecognizer.authorizationStatus()),
        "on_device": recognizer?.supportsOnDeviceRecognition ?? false,
        "language": recognizer?.locale.identifier ?? "",
    ])
    exit(0)
}

let meeting = arguments.contains("--meeting")
// The second copy, started by `--meeting`: the Mac's sound only.
let systemOnly = arguments.contains("--system")
// `--from-app <pid>` (a video): only the sound of that app.
var fromApp: pid_t?
if let i = arguments.firstIndex(of: "--from-app"), i + 1 < arguments.count, let pid = Int32(arguments[i + 1]), pid > 0 {
    fromApp = pid
}
var soundFile: String?
if let i = arguments.firstIndex(of: "--file"), i + 1 < arguments.count {
    soundFile = arguments[i + 1]
}

// A pipe that the other end has left must not end this program at once.
signal(SIGPIPE, SIG_IGN)

// Ask for what is needed, in the order it is used.
if !ask({ answer in SFSpeechRecognizer.requestAuthorization { answer($0 == .authorized) } }) {
    fail("speech", "Speech recognition is not allowed for Rune. Turn it on in System Settings, Privacy & Security, Speech Recognition.")
}
if soundFile == nil, !systemOnly, !ask({ answer in AVCaptureDevice.requestAccess(for: .audio) { answer($0) } }) {
    fail("microphone", "The microphone is not allowed for Rune. Turn it on in System Settings, Privacy & Security, Microphone.")
}

guard let me = Transcriber(who: systemOnly ? "others" : "me", locale: locale) else {
    fail("language", "Speech recognition has no support for this Mac's language.")
}
guard me.recognizer.isAvailable else {
    fail("speech", "Speech recognition is not available right now.")
}

// A sound file in place of the microphone: its sound goes in at the speed it
// was spoken, then the last words become final.
if let soundFile {
    guard let file = try? AVAudioFile(forReading: URL(fileURLWithPath: soundFile)) else {
        fail("file", "Couldn't read the sound file.")
    }
    me.begin()
    emit(["t": "status", "state": "listening", "on_device": me.onDevice, "others": false, "language": me.recognizer.locale.identifier])
    DispatchQueue.global().async {
        Thread.sleep(forTimeInterval: 0.3)
        let format = file.processingFormat
        while file.framePosition < file.length {
            guard let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: 4096), (try? file.read(into: buffer)) != nil, buffer.frameLength > 0 else { break }
            me.append(buffer)
            Thread.sleep(forTimeInterval: Double(buffer.frameLength) / format.sampleRate)
        }
        me.end {
            emit(["t": "done"])
            exit(0)
        }
    }
    dispatchMain()
}

let microphone = Microphone()
let systemSound = SystemSound()
var others: Others?

me.begin()
if systemOnly {
    do {
        try systemSound.start(app: fromApp) { me.append($0) }
    } catch {
        systemSound.stop()
        fail("system-audio", error.localizedDescription)
    }
} else {
    do {
        try microphone.start { me.append($0) }
    } catch {
        fail("microphone", error.localizedDescription)
    }
}

var hearsOthers = systemOnly
if meeting {
    var extra: [String] = []
    if let i = arguments.firstIndex(of: "--locale"), i + 1 < arguments.count {
        extra += ["--locale", arguments[i + 1]]
    }
    if debugging {
        extra.append("--debug")
    }
    let second = Others()
    if let problem = second.start(extra) {
        // Carry on with the microphone alone.
        emit(["t": "note", "what": "system-audio", "message": "Only your microphone is heard. \(problem)"])
    } else {
        others = second
        hearsOthers = true
    }
}

emit(["t": "status", "state": "listening", "on_device": me.onDevice, "others": hearsOthers, "language": me.recognizer.locale.identifier])
if debugging {
    NotificationCenter.default.addObserver(forName: .AVAudioEngineConfigurationChange, object: nil, queue: nil) { _ in
        debug(["what": "The microphone's set-up changed."])
    }
}

let stopLock = NSLock()
var stopped = false

func shutDown() {
    stopLock.lock()
    let already = stopped
    stopped = true
    stopLock.unlock()
    if already { return }
    if systemOnly {
        systemSound.stop()
    } else {
        microphone.stop()
    }
    let group = DispatchGroup()
    group.enter()
    me.end { group.leave() }
    if let others {
        group.enter()
        DispatchQueue.global().async {
            others.stop()
            group.leave()
        }
    }
    _ = group.wait(timeout: .now() + 5)
    emit(["t": "done"])
    exit(0)
}

// The app says "stop", or closes our input, or ends us.
DispatchQueue.global().async {
    while let line = readLine() {
        if line.trimmingCharacters(in: .whitespaces) == "stop" { break }
    }
    shutDown()
}
signal(SIGTERM, SIG_IGN)
let terminate = DispatchSource.makeSignalSource(signal: SIGTERM, queue: .global())
terminate.setEventHandler { shutDown() }
terminate.resume()

dispatchMain()
