import AppKit
import Carbon
import SwiftUI

// MARK: - Root

struct AppSettingsView: View {
    @ObservedObject private var settings = AppSettings.shared
    @State private var selectedTab: SettingsTab = .general

    enum SettingsTab: String, CaseIterable, Identifiable {
        case general, about
        var id: String { rawValue }
        var label: String {
            switch self {
            case .general: L.generalTab
            case .about:   L.aboutTab
            }
        }
    }

    var body: some View {
        HStack(spacing: 0) {
            sidebar
            Divider()
            detail
        }
        .frame(width: 625, height: 480)
    }

    // MARK: Sidebar

    private var sidebar: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(L.appSettingsTitle)
                .font(Font.custom("LXGWWenKai-Medium", size: 17))
                .padding(.horizontal, 16)
                .padding(.top, 20)
                .padding(.bottom, 12)

            ForEach(SettingsTab.allCases) { tab in
                sidebarItem(tab)
            }

            Spacer()
        }
        .frame(width: 160)
        .background(Color(NSColor.windowBackgroundColor))
    }

    private func sidebarItem(_ tab: SettingsTab) -> some View {
        Button(action: { selectedTab = tab }) {
            Text(tab.label)
                .font(Font.custom("LXGWWenKai-Medium", size: 14))
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 12)
                .padding(.vertical, 7)
                .background(
                    selectedTab == tab
                        ? RoundedRectangle(cornerRadius: 6).fill(Color.blue.opacity(0.8))
                        : RoundedRectangle(cornerRadius: 6).fill(Color.clear)
                )
                .foregroundColor(selectedTab == tab ? .white : .primary)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .padding(.horizontal, 8)
    }

    // MARK: Detail

    @ViewBuilder
    private var detail: some View {
        switch selectedTab {
        case .general: GeneralPane(settings: settings).id(settings.appLanguage)
        case .about:   AboutPane()
        }
    }
}

// MARK: - General Pane

// Reads the Picker's actual rendered width so the hotkey badge can match it exactly.
private struct PickerWidthKey: PreferenceKey {
    static var defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
        value = max(value, nextValue())
    }
}

private struct GeneralPane: View {
    @ObservedObject var settings: AppSettings
    @State private var pickerWidth: CGFloat = 0
    private let labelW: CGFloat = 92  // wide enough for "Language：" in English

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(L.generalTab)
                .font(Font.custom("LXGWWenKai-Medium", size: 17))
                .padding(.bottom, 6)

            // Language
            HStack(alignment: .center, spacing: 8) {
                Text(L.languageLabel + "：")
                    .font(Font.custom("LXGWWenKai-Medium", size: 14))
                    .frame(width: labelW, alignment: .trailing)
                Picker("", selection: $settings.appLanguage) {
                    ForEach(AppLanguage.allCases) { lang in
                        Text(lang.displayName)
                            .font(Font.custom("LXGWWenKai-Medium", size: 14))
                            .tag(lang)
                    }
                }
                .labelsHidden()
                // Measure the picker's actual rendered width
                .overlay(GeometryReader { geo in
                    Color.clear.preference(key: PickerWidthKey.self, value: geo.size.width)
                })
            }

            // Hotkey
            HStack(alignment: .center, spacing: 8) {
                Text(L.hotkeyLabel + "：")
                    .font(Font.custom("LXGWWenKai-Medium", size: 14))
                    .frame(width: labelW, alignment: .trailing)
                HotkeyControls(settings: settings, badgeWidth: pickerWidth)
            }

            // Auto update
            HStack(alignment: .center, spacing: 8) {
                Color.clear.frame(width: labelW)
                Toggle("", isOn: $settings.autoUpdate)
                    .labelsHidden()
                    .toggleStyle(.checkbox)
                Text(L.autoUpdate)
                    .font(Font.custom("LXGWWenKai-Medium", size: 14))
                    .fixedSize()
            }

            // Launch at login
            HStack(alignment: .center, spacing: 8) {
                Color.clear.frame(width: labelW)
                Toggle("", isOn: $settings.launchAtLogin)
                    .labelsHidden()
                    .toggleStyle(.checkbox)
                Text(L.launchAtLogin)
                    .font(Font.custom("LXGWWenKai-Medium", size: 14))
                    .fixedSize()
            }
        }
        // fixedSize prevents SwiftUI from distributing the parent's extra height
        // to flexible children (Text), which would push Launch at Login to the bottom
        .fixedSize(horizontal: false, vertical: true)
        .padding(.horizontal, 24)
        .padding(.top, 24)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .onPreferenceChange(PickerWidthKey.self) { w in
            if w > 0 { pickerWidth = w }
        }
    }
}

// MARK: - Hotkey Controls

private struct HotkeyControls: View {
    @ObservedObject var settings: AppSettings
    let badgeWidth: CGFloat
    @State private var isRecording = false
    @State private var monitor: Any?
    @State private var isHoveringBadge = false
    @State private var isHoveringReset = false

    private let badgeHeight: CGFloat = 28

    var body: some View {
        HStack(spacing: 8) {
            // Badge — click to record; width matches the language picker exactly
            Text(isRecording ? L.hotkeyRecording : settings.hotkeyDisplayString)
                .font(isRecording
                    ? Font.custom("LXGWWenKai-Medium", size: 14)
                    : Font.custom("MapleMono-NF-CN-Regular", size: 15))
                .foregroundColor(isRecording ? Color.accentColor : .primary)
                .frame(width: badgeWidth > 0 ? badgeWidth : 140, height: badgeHeight, alignment: .center)
                .background(
                    RoundedRectangle(cornerRadius: 7)
                        .strokeBorder(
                            isRecording ? Color.accentColor : Color(NSColor.separatorColor),
                            lineWidth: isRecording ? 1.5 : 1
                        )
                        .background(
                            RoundedRectangle(cornerRadius: 7)
                                .fill(isRecording
                                    ? (isHoveringBadge
                                        ? Color.accentColor.opacity(0.15)
                                        : Color.accentColor.opacity(0.08))
                                    : (isHoveringBadge
                                        ? Color(NSColor.controlColor)
                                        : Color(NSColor.controlColor).opacity(0.0)))
                        )
                )
                .onTapGesture { isRecording ? stopRecording() : startRecording() }
                .onHover { isHoveringBadge = $0 }

            if isRecording {
                Button { stopRecording() } label: {
                    Text(L.hotkeyCancel)
                        .font(Font.custom("LXGWWenKai-Medium", size: 13))
                }
                .controlSize(.small)
            } else {
                // Reset — icon with border
                Button { settings.resetHotkey() } label: {
                    Image(systemName: "arrow.counterclockwise")
                        .font(.system(size: 12, weight: .medium))
                        .foregroundColor(isHoveringReset ? .primary : .secondary)
                        .frame(width: badgeHeight, height: badgeHeight)
                        .background(
                            RoundedRectangle(cornerRadius: 6)
                                .strokeBorder(Color(NSColor.separatorColor), lineWidth: 1)
                                .background(
                                    RoundedRectangle(cornerRadius: 6)
                                        .fill(isHoveringReset
                                            ? Color(NSColor.controlColor).opacity(0.85)
                                            : Color(NSColor.controlColor))
                                )
                        )
                }
                .buttonStyle(.plain)
                .onHover { isHoveringReset = $0 }
                .help(L.hotkeyReset)
            }
        }
    }

    private func startRecording() {
        isRecording = true
        monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
            if Int(event.keyCode) == kVK_Escape {
                self.stopRecording()
                return nil
            }
            let keyCode = UInt32(event.keyCode)
            let mods    = HotkeyManager.carbonModifiers(from: event.modifierFlags)
            if mods != 0 {
                // Assign separately to avoid double re-register from didSet
                self.settings.hotkeyKeyCode = keyCode
                self.settings.hotkeyCarbonModifiers = mods
            }
            self.stopRecording()
            return nil
        }
    }

    private func stopRecording() {
        isRecording = false
        if let m = monitor { NSEvent.removeMonitor(m); monitor = nil }
    }
}

// MARK: - About Pane

private struct AboutPane: View {
    @State private var isHoveringName = false
    @State private var isHoveringGithub = false

    private var versionString: String {
        let v = Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "—"
        return "\(L.aboutVersion)  \(v)"
    }

    var body: some View {
        VStack(spacing: 0) {
            Text("MiniNotes")
                .font(Font.custom("LXGWWenKai-Medium", size: 36))
                .padding(.bottom, 8)

            Text(L.aboutDescription)
                .font(Font.custom("LXGWWenKai-Medium", size: 13))
                .foregroundColor(.secondary)
                .multilineTextAlignment(.center)
                .padding(.bottom, 12)

            HStack(spacing: 4) {
                Text(L.aboutMadeBy)
                    .font(Font.custom("LXGWWenKai-Medium", size: 13))
                    .foregroundColor(.secondary)

                Link("Pingfan Hu", destination: URL(string: "https://pingfanhu.com")!)
                    .font(Font.custom("LXGWWenKai-Medium", size: 13))
                    .foregroundColor(Color(red: 0.42, green: 0.50, blue: 0.77))
                    .underline(isHoveringName, color: Color(red: 0.42, green: 0.50, blue: 0.77))
                    .onHover { hovering in
                        withAnimation(.easeInOut(duration: 0.15)) { isHoveringName = hovering }
                        hovering ? NSCursor.pointingHand.push() : NSCursor.pop()
                    }

                Text(L.aboutMadeByAfter)
                    .font(Font.custom("LXGWWenKai-Medium", size: 13))
                    .foregroundColor(.secondary)
            }
            .padding(.bottom, 8)

            Text(versionString)
                .font(Font.custom("LXGWWenKai-Medium", size: 13))
                .foregroundColor(.secondary)
                .padding(.bottom, 16)

            Link(destination: URL(string: "https://github.com/alkaline-software/MiniNotes")!) {
                Image("github-logo")
                    .resizable()
                    .scaledToFit()
                    .frame(width: 55, height: 55)
                    .clipShape(Circle())
                    .opacity(isHoveringGithub ? 1.0 : 0.75)
                    .scaleEffect(isHoveringGithub ? 1.05 : 1.0)
            }
            .buttonStyle(.plain)
            .help("GitHub")
            .onHover { hovering in
                withAnimation(.easeInOut(duration: 0.15)) { isHoveringGithub = hovering }
                hovering ? NSCursor.pointingHand.push() : NSCursor.pop()
            }

        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        .padding(.top, 32)
    }
}
