// The frame of the app: the four tabs, the glass bar at the bottom, and the
// first screen before any notes are loaded.

import Notion2Core
import SwiftUI
import UniformTypeIdentifiers

struct RootView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.scenePhase) private var scenePhase
    @State private var router = Router()
    @State private var sync = SyncCoordinator()
    @AppStorage("appearance") private var appearance = "system"

    var body: some View {
        @Bindable var model = model
        Group {
            switch model.phase {
            case .loading:
                AppBackground()
            case .empty:
                WelcomeView()
            case .ready:
                tabs
            }
        }
        .environment(router)
        .environment(sync)
        .tint(Theme.accent)
        .preferredColorScheme(appearance == "light" ? .light : appearance == "dark" ? .dark : nil)
        .onOpenURL { url in
            if url.isFileURL {
                Task { await model.loadCopy(from: url) }
            } else if PairingLink.isOne(url) {
                // The camera read the QR code that the Mac shows.
                sync.opened(url)
            } else if url.scheme == "rune" || url.scheme == "notion2", url.host == "page", let id = url.pathComponents.dropFirst().first,
                      model.workspace.pages[id] != nil {
                // A link to a page of Rune ("rune://page/<id>", or "notion2://" from before 0.1.9, as the
                // Mac copies it).
                router.openPage(id)
            }
        }
        .onChange(of: model.generation) {
            router.reset()
        }
        // Sync runs only while the app is in front.
        .onChange(of: scenePhase, initial: true) { _, phase in
            if phase == .active {
                model.onChange = { [sync] in sync.changed() }
                sync.activate(model: model)
            } else if phase == .background {
                sync.pause()
            }
        }
        .alert("Connect to \(sync.pendingLink?.name ?? "your Mac")?", isPresented: Binding(
            get: { sync.pendingLink != nil },
            set: { if !$0 { sync.pendingLink = nil } }
        ), presenting: sync.pendingLink) { link in
            Button("Connect") {
                Task { await sync.pair(link) }
            }
            Button("Cancel", role: .cancel) {}
        } message: { _ in
            Text("This iPhone then gets its notes from that Mac over your Wi-Fi.")
        }
        .alert("Could not connect", isPresented: Binding(
            get: { sync.pairError != nil },
            set: { if !$0 { sync.pairError = nil } }
        )) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(sync.pairError ?? "")
        }
        .alert("This file cannot be shown", isPresented: Binding(
            get: { model.loadError != nil },
            set: { if !$0 { model.loadError = nil } }
        )) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(model.loadError ?? "")
        }
        .alert("The change was not saved", isPresented: Binding(
            get: { model.saveError != nil },
            set: { if !$0 { model.saveError = nil } }
        )) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(model.saveError ?? "")
        }
    }

    /// The tabs. The system's own tab bar is hidden: each first screen
    /// carries the glass bar of the designs (`TabBar`, `SearchBar`).
    private var tabs: some View {
        @Bindable var router = router
        return TabView(selection: $router.tab) {
            ForEach(AppTab.allCases) { tab in
                TabStack(tab: tab)
                    .tag(tab)
                    .toolbar(.hidden, for: .tabBar)
            }
        }
        // A new copy of the notes starts every screen again.
        .id(model.generation)
    }
}

/// One tab: its first screen and the pages opened on top of it.
private struct TabStack: View {
    let tab: AppTab
    @Environment(Router.self) private var router

    var body: some View {
        NavigationStack(path: router.path(for: tab)) {
            Group {
                switch tab {
                case .home: HomeView()
                case .pages: PagesView()
                case .search: SearchView()
                case .calendar: CalendarView()
                }
            }
            .navigationDestination(for: Route.self) { route in
                switch route {
                case let .page(id): PageView(pageID: id)
                case .trash: TrashView()
                case .todos: TodosView()
                }
            }
        }
    }
}

/// The glass bar with the four tabs, at the bottom of a tab's first screen.
struct TabBar: View {
    @Environment(Router.self) private var router

    var body: some View {
        HStack(spacing: 2) {
            ForEach(AppTab.allCases) { tab in
                tabButton(tab)
            }
        }
        .padding(5)
        .frame(height: 62)
        .glassEffect(.regular, in: .capsule)
        .padding(.horizontal, 16)
        .padding(.top, 8)
        // The designs put the bar a little into the home-indicator area.
        .padding(.bottom, -6)
    }

    private func tabButton(_ tab: AppTab) -> some View {
        let selected = router.tab == tab
        return Button {
            router.tab = tab
        } label: {
            VStack(spacing: 2) {
                IconView(tab.icon, size: 23, weight: selected ? 1.9 : 1.75)
                Text(tab.title)
                    .font(.system(size: 10.5, weight: selected ? .semibold : .medium))
            }
            .foregroundStyle(selected ? Theme.text : Theme.text2)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background {
                if selected {
                    Capsule().fill(Theme.lens)
                }
            }
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

/// The bar of the Search screen: a button that closes search, and the field.
struct SearchBar: View {
    @Environment(Router.self) private var router
    @FocusState private var typing: Bool

    var body: some View {
        @Bindable var router = router
        GlassEffectContainer(spacing: 10) {
            HStack(spacing: 10) {
                Button {
                    typing = false
                    router.closeSearch()
                } label: {
                    IconView("close", size: 22, weight: 1.9)
                        .frame(width: 56, height: 56)
                        .contentShape(Circle())
                }
                .buttonStyle(.plain)
                .glassEffect(.regular.interactive(), in: .circle)
                .accessibilityLabel("Close search")

                HStack(spacing: 10) {
                    IconView("search", size: 20)
                        .foregroundStyle(Theme.text2)
                    TextField("Search", text: $router.query)
                        .font(.system(size: 17))
                        .focused($typing)
                        .submitLabel(.search)
                        .autocorrectionDisabled()
                        .textInputAutocapitalization(.never)
                        .onSubmit { typing = false }
                    if !router.query.isEmpty {
                        Button {
                            router.query = ""
                            typing = true
                        } label: {
                            Image(systemName: "xmark.circle.fill")
                                .font(.system(size: 17))
                                .foregroundStyle(Theme.text2)
                                .frame(width: 32, height: 44)
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel("Clear")
                    }
                }
                .padding(.leading, 18)
                .padding(.trailing, router.query.isEmpty ? 18 : 8)
                .frame(height: 56)
                .glassEffect(.regular, in: .capsule)
            }
        }
        .foregroundStyle(Theme.text)
        .padding(.horizontal, 16)
        .padding(.top, 8)
        .padding(.bottom, typing ? 8 : -3)
        .onChange(of: router.tab, initial: true) { _, tab in
            // Opening Search puts the cursor in the field.
            if tab == .search && router.query.isEmpty {
                typing = true
            }
        }
    }
}

/// The first screen, before a copy of the notes is on the phone.
struct WelcomeView: View {
    @Environment(AppModel.self) private var model
    @Environment(SyncCoordinator.self) private var sync
    @State private var picking = false

    /// What to do next, which depends on what this iPhone can already use.
    private var advice: String {
        if let name = sync.connectingTo {
            return "Connecting to \(name)…"
        }
        if let mac = sync.mac {
            return switch sync.wifi {
            case .away: "Waiting for \(mac.name). Put this iPhone and the Mac on the same Wi-Fi, and keep Rune open on the Mac."
            case .refused, .notTheMac: "\(mac.name) no longer knows this iPhone. Show a new code on the Mac and point the camera at it."
            default: "Getting your notes from \(mac.name)…"
            }
        }
        let code = "show the code for an iPhone in Rune on your Mac and point the camera of this iPhone at it."
        return switch sync.status {
        case .unavailable, .failed:
            "To get your notes over Wi-Fi, \(code) You can also load a copy from the Mac."
        default:
            "This iPhone is ready for sync through iCloud Drive. To get your notes over Wi-Fi, \(code)"
        }
    }

    var body: some View {
        VStack(spacing: 14) {
            Spacer()
            IconView("note", size: 44, weight: 1.5)
                .foregroundStyle(Theme.text2)
            Text("No notes yet")
                .font(.system(size: 24, weight: .bold))
                .foregroundStyle(Theme.text)
            Text(advice)
                .font(.system(size: 15))
                .foregroundStyle(Theme.text2)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 36)
            Button("Load a copy…") { picking = true }
                .buttonStyle(.glassProminent)
                .controlSize(.large)
                .tint(Theme.ink)
                .foregroundStyle(Theme.onInk)
                .padding(.top, 8)
            Spacer()
            Spacer()
        }
        .frame(maxWidth: .infinity)
        .background { AppBackground() }
        .workspaceImporter(isPresented: $picking)
    }
}

extension View {
    /// The Files picker for a workspace file.
    func workspaceImporter(isPresented: Binding<Bool>) -> some View {
        modifier(WorkspaceImporter(isPresented: isPresented))
    }
}

private struct WorkspaceImporter: ViewModifier {
    @Binding var isPresented: Bool
    @Environment(AppModel.self) private var model

    func body(content: Content) -> some View {
        content.fileImporter(isPresented: $isPresented, allowedContentTypes: [.data]) { result in
            if case let .success(url) = result {
                Task { await model.loadCopy(from: url) }
            }
        }
    }
}
