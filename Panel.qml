import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui
import "Model.js" as Model

Panel {
  id: root

  moduleName: "keepassxc.control"
  ipcTarget: "keepassxc.control"
  manageIpc: false

  property string searchText: ""
  property string setupPathText: ""
  property string setupPasswordText: ""
  property int selectedIndex: 0
  property bool preferPassword: false
  property bool fingerprintAttempted: false
  property bool resumeConfig: false
  property string lastVaultState: ""
  property string lastAction: ""
  readonly property string fingerprintPrompt: "Put your finger on the sensor"

  property bool clipboardHot: false
  property real clipboardRemainingSeconds: 0
  property string clipboardExpected: ""
  property real clipboardDeadlineMs: 0
  property string clipboardEntryId: ""
  property string pendingClipboardEntryId: ""
  property string pendingClipboardText: ""
  property string pendingClipboardLabel: ""
  property string detailEntryId: ""
  property string detailUsername: ""
  property string detailPassword: ""
  property string detailUrl: ""
  property string detailNotes: ""
  property bool detailsLoaded: false
  property bool passwordRevealed: false

  readonly property color foreground: bar ? bar.foreground : Color.foreground
  readonly property color urgent: bar ? bar.urgent : Color.urgent
  readonly property color dim: Qt.darker(foreground, 1.55)
  readonly property string fontFamily: bar ? bar.fontFamily : Style.font.family
  readonly property int clipboardTimeoutSec: Model.intSetting(settings, "clipboardTimeoutSec", 30, 5, 120)
  readonly property string screen: Model.visibleScreen({
    path: String(root.setupPathText || vault.databasePath || ""),
    unlocked: vault.unlocked && vault.state === "unlocked",
    hasStoredSecret: vault.hasStoredSecret,
    preferPassword: root.preferPassword,
    openFailed: vault.openFailed,
    fingerprintAvailable: vault.fingerprintAvailable
  })
  readonly property bool ready: root.screen === "list"
  readonly property var selectedEntry: entryAt(selectedIndex)

  function entryAt(index) {
    if (index < 0 || index >= entriesModel.count) return null
    return entriesModel.get(index)
  }

  function beginOpen() {
    var path = root.setupPathText !== "" ? root.setupPathText : vault.databasePath
    var stayInConfig = root.resumeConfig
    root.resumeConfig = false
    root.setupPathText = path
    root.setupPasswordText = ""
    setupPathField.text = path
    setupPasswordField.text = ""
    searchField.text = root.searchText
    root.preferPassword = stayInConfig
    root.fingerprintAttempted = stayInConfig
    vault.errorText = ""
    if (!stayInConfig && !vault.openFailed && vault.hasStoredSecret && vault.fingerprintAvailable)
      root.lastAction = root.fingerprintPrompt
    root.rebuildEntries()
    vault.refresh()
    root.focusCurrent()
    if (!stayInConfig && !vault.openFailed) root.maybeFingerprintUnlock()
  }

  function focusCurrent() {
    if (!root.opened) return
    Qt.callLater(function() {
      if (!root.opened) return
      if (root.ready) {
        searchField.forceActiveFocus()
        return
      }
      if (root.screen === "config" && root.setupPathText === "") setupPathField.forceActiveFocus()
      else if (root.screen === "config") setupPasswordField.forceActiveFocus()
    })
  }

  onReadyChanged: {
    if (opened && ready) root.focusCurrent()
  }

  function rebuildEntries() {
    var filtered = Model.filterEntries(Model.safeEntries(vault.entries), root.searchText)
    entriesModel.clear()
    for (var i = 0; i < filtered.length; i++) {
      entriesModel.append({
        entryId: filtered[i].id,
        entryTitle: filtered[i].title,
        entryUsername: filtered[i].username
      })
    }
    selectedIndex = Model.nextSelectedIndex(selectedIndex, entriesModel.count)
    root.loadSelectedDetails()
  }

  onSelectedIndexChanged: root.loadSelectedDetails()

  function loadSelectedDetails() {
    var entry = root.entryAt(root.selectedIndex)
    if (!entry) {
      root.clearDetails()
      return
    }
    var id = String(entry.entryId || "")
    var username = String(entry.entryUsername || "")
    root.detailUsername = username
    if (id === "") {
      root.clearDetails()
      return
    }
    if (id === root.detailEntryId && root.detailsLoaded) return
    root.detailEntryId = id
    root.detailPassword = ""
    root.detailUrl = ""
    root.detailNotes = ""
    root.detailsLoaded = false
    root.passwordRevealed = false
    vault.readDetails(id)
  }

  function copyDetail(field) {
    var value = ""
    var label = ""
    if (field === "username") {
      value = root.detailUsername
      label = "Username copied"
    } else if (field === "password") {
      value = root.detailPassword
      label = "Password copied"
    } else if (field === "url") {
      value = root.detailUrl
      label = "URL copied"
    } else if (field === "notes") {
      value = root.detailNotes
      label = "Notes copied"
    } else {
      return
    }
    if (String(value) === "") return
    root.pendingClipboardEntryId = root.detailEntryId
    root.writeClipboard(value, label)
  }

  function openSelectedUrl() {
    var raw = String(root.detailUrl || "").trim()
    if (raw === "") return
    var url = raw
    if (!/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(url))
      url = "https://" + url
    if (!/^https?:\/\//i.test(url)) {
      root.lastAction = "Only http and https URLs can be opened"
      return
    }
    Quickshell.execDetached(["xdg-open", url])
    root.lastAction = "Opening URL"
  }

  function select(delta) {
    if (entriesModel.count === 0) return
    selectedIndex = (selectedIndex + delta + entriesModel.count) % entriesModel.count
    resultList.positionViewAtIndex(selectedIndex, ListView.Contain)
  }

  function readSelected() {
    var entry = root.entryAt(root.selectedIndex)
    if (!root.ready || !entry) return
    var id = String(entry.entryId || "")
    root.pendingClipboardEntryId = id
    if (root.detailsLoaded && root.detailEntryId === id && String(root.detailPassword) !== "") {
      root.writeClipboard(root.detailPassword, "Password copied")
      return
    }
    root.lastAction = "Reading password…"
    vault.readField(id)
  }

  function resetListUiState() {
    root.searchText = ""
    if (searchField) searchField.text = ""
    root.selectedIndex = 0
  }

  function clearDetails() {
    root.detailEntryId = ""
    root.detailUsername = ""
    root.detailPassword = ""
    root.detailUrl = ""
    root.detailNotes = ""
    root.detailsLoaded = false
    root.passwordRevealed = false
  }

  function persistDatabasePath(path, source) {
    var next = String(path || "")
    if (!Model.shouldPersistDatabasePath(Model.setting(settings, "databasePath", ""), next, {
      unlocking: vault.unlocking,
      source: source
    })) return
    root.resetListUiState()
    vault.openFailed = false
    if (!root.bar || !root.bar.shell || typeof root.bar.shell.updateEntryInline !== "function") return
    var entry = { id: root.moduleName }
    for (var key in root.settings) if (key !== "id") entry[key] = root.settings[key]
    entry.databasePath = next
    root.bar.shell.updateEntryInline(root.moduleName, entry)
  }

  function stayOnConfig() {
    root.resumeConfig = true
    root.preferPassword = true
    root.fingerprintAttempted = true
  }

  function browseDatabase() {
    root.stayOnConfig()
    root.close()
    vault.pickDatabaseFile()
  }

  function openDatabaseSettings() {
    root.preferPassword = true
    root.fingerprintAttempted = true
    root.lastAction = ""
    if (root.ready || vault.state === "unlocked") vault.lock()
    Qt.callLater(function() {
      if (root.opened) setupPasswordField.forceActiveFocus()
    })
  }

  function unlock() {
    var path = String(root.setupPathText || "").trim()
    var password = String(root.setupPasswordText || "")
    if (path === "" || password === "") {
      root.lastAction = "Database path and password are required"
      return
    }
    root.lastAction = "Opening database…"
    vault.unlock(path, password)
    root.setupPasswordText = ""
    setupPasswordField.text = ""
  }

  function unlockWithFingerprint() {
    var path = String(root.setupPathText || "").trim()
    if (path === "") {
      root.lastAction = "Database path is required"
      return
    }
    if (vault.state === "authenticating") return
    root.fingerprintAttempted = true
    root.lastAction = root.fingerprintPrompt
    vault.unlock(path, "")
  }

  function maybeFingerprintUnlock() {
    if (!root.opened || root.ready || root.preferPassword || root.fingerprintAttempted) return
    if (vault.openFailed || !vault.hasStoredSecret || !vault.fingerprintAvailable) return
    if (vault.state === "authenticating") return
    if (root.screen !== "fingerprint") return
    root.unlockWithFingerprint()
  }

  function openGui() {
    Quickshell.execDetached([
      "omarchy-launch-or-focus",
      "org.keepassxc.KeePassXC",
      "uwsm-app -- gtk-launch org.keepassxc.KeePassXC.desktop"
    ])
  }

  function writeClipboard(value, label) {
    var text = String(value === undefined || value === null ? "" : value)
    if (text === "") return
    root.pendingClipboardText = text
    root.pendingClipboardLabel = String(label || "Copied to clipboard")
    vault.writeClipboard(text, root.pendingClipboardEntryId)
  }

  function startClipboardTimer() {
    root.clipboardHot = true
    root.clipboardDeadlineMs = Date.now() + root.clipboardTimeoutSec * 1000
    root.clipboardRemainingSeconds = root.clipboardTimeoutSec
    clipboardTimer.restart()
  }

  function clipboardText(value) {
    var text = String(value === undefined || value === null ? "" : value)
    if (text.length > 0 && text.charAt(text.length - 1) === "\n")
      return text.slice(0, -1)
    return text
  }

  function finishClipboardCheck(currentValue) {
    var unreadable = currentValue === undefined || currentValue === null
    if (!unreadable && root.clipboardText(currentValue) !== root.clipboardText(root.clipboardExpected)) {
      root.clipboardHot = false
      root.clipboardRemainingSeconds = 0
      root.clipboardExpected = ""
      root.clipboardEntryId = ""
      return
    }
    Quickshell.execDetached(["/usr/bin/wl-copy", "--clear"])
    root.clearClipboardFinished(0)
  }

  function clearClipboardFinished(exitCode) {
    root.clipboardHot = false
    root.clipboardRemainingSeconds = 0
    root.clipboardExpected = ""
    root.clipboardEntryId = ""
    if (exitCode !== 0) root.lastAction = "Could not clear the clipboard"
  }

  onOpenedChanged: {
    if (opened) beginOpen()
    else {
      if (vault.state === "authenticating") vault.lock()
      root.clearDetails()
    }
  }

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  Service {
    id: vault
    settings: root.settings
  }

  Connections {
    target: vault
    function onEntriesChanged() {
      root.rebuildEntries()
      if (root.opened && root.ready) root.focusCurrent()
    }
    function onFieldRead(value, entryId) {
      if (entryId) root.pendingClipboardEntryId = entryId
      root.writeClipboard(value, "Password copied")
    }
    function onClipboardWritten(entryId) {
      root.clipboardExpected = root.pendingClipboardText
      root.clipboardEntryId = entryId || root.pendingClipboardEntryId
      root.startClipboardTimer()
      root.lastAction = root.pendingClipboardLabel
      root.pendingClipboardText = ""
      root.pendingClipboardLabel = ""
    }
    function onDetailsRead(entryId, username, password, url, notes) {
      if (root.detailEntryId !== entryId) return
      root.detailUsername = username
      root.detailPassword = password
      root.detailUrl = url
      root.detailNotes = notes
      root.detailsLoaded = true
    }
    function onHelperFailed(message) {
      var text = String(message || "")
      if (vault.state === "setup" && vault.hasStoredSecret)
        root.preferPassword = true
      root.lastAction = text
      errorBlink.restart()
    }
    function onAuthProgress(message) {
      var text = String(message || "")
      if (text !== "") root.lastAction = text
    }
    function onIdleLocked() {
      if (root.opened) root.close()
    }
    function onHasStoredSecretChanged() { root.maybeFingerprintUnlock() }
    function onFingerprintAvailableChanged() { root.maybeFingerprintUnlock() }
    function onStateChanged() {
      var wasAuthenticating = root.lastVaultState === "authenticating"
      if (vault.state === "setup")
        root.preferPassword = true
      if (root.lastVaultState === "unlocked" && vault.state === "locked")
        root.fingerprintAttempted = false
      if (vault.state === "unlocked") {
        root.preferPassword = false
        if (root.lastAction === "Opening database…")
          root.lastAction = ""
        var persistPath = String(root.setupPathText || vault.databasePath || "")
        if (persistPath !== "")
          root.persistDatabasePath(persistPath, "unlock")
      }
      root.lastVaultState = vault.state
      if (wasAuthenticating && vault.state === "locked" && root.opened && !root.preferPassword) {
        root.lastAction = "Try again"
        return
      }
      root.maybeFingerprintUnlock()
    }
    function onFilePicked(path) {
      root.setupPathText = path
      root.stayOnConfig()
      if (!root.opened) root.open()
      else {
        setupPathField.text = path
        setupPasswordField.forceActiveFocus()
      }
    }
    function onFilePickCancelled() {
      root.stayOnConfig()
      if (!root.opened) root.open()
    }
    function onFilePickFailed(message) {
      root.stayOnConfig()
      root.lastAction = message
      if (!root.opened) root.open()
    }
  }

  IpcHandler {
    target: root.ipcTarget
    function open(): void { root.open() }
    function close(): void { root.close() }
    function show(): void { root.open() }
    function hide(): void { root.close() }
    function toggle(): void { root.toggle() }
    function lock(): string { vault.lock(); return "ok" }
    function status(): string { return vault.state }
  }

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: (root.ready ? "󰍁" : "󰌾") + (root.clipboardHot ? " •" : "")
    tooltipText: (root.ready ? "KeePassXC unlocked" : "KeePassXC locked")
      + (root.clipboardHot ? " · clipboard hot" : "")
    slotSize: Style.bar.statusSlot
    onPressed: function(buttonCode) {
      if (buttonCode === Qt.RightButton) vault.lock()
      else if (buttonCode === Qt.MiddleButton) root.openGui()
      else root.toggle()
    }
  }

  KeyboardPanel {
    id: panel
    anchorItem: button
    owner: root
    bar: root.bar
    open: root.opened
    focusTarget: root.screen === "list" ? searchField
                 : root.screen === "config" ? (root.setupPathText === "" ? setupPathField : setupPasswordField)
                 : null
    contentWidth: panel.fittedContentWidth(Style.space(416))
    contentHeight: panel.fittedContentHeight(contentColumn.implicitHeight, Style.space(620))

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      blocked: searchField.activeFocus || setupPathField.activeFocus || setupPasswordField.activeFocus
      onMoveRequested: function(dx, dy) {
        if (dy > 0) root.select(1)
        else if (dy < 0) root.select(-1)
      }
      onActivateRequested: root.readSelected()
      onCloseRequested: root.close()
      onTabRequested: function(direction) { root.switchPanel(direction) }
      onTextKey: function(t) {
        if (t === "g" || t === "G") root.openGui()
        else if (t === "b" || t === "B") root.copyDetail("username")
      }

      Flickable {
        id: panelFlick
        anchors.fill: parent
        contentWidth: width
        contentHeight: contentColumn.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        flickableDirection: Flickable.VerticalFlick
        interactive: !root.ready && contentHeight > height
        ScrollBar.vertical: ScrollBar {
          policy: root.ready ? ScrollBar.AlwaysOff : ScrollBar.AsNeeded
        }

        Column {
          id: contentColumn
          width: panelFlick.width
          spacing: Style.space(12)

          PanelHero {
            width: parent.width
            title: "KeePassXC"
            meta: root.ready
              ? (root.clipboardHot && root.lastAction !== "" ? root.lastAction : entriesModel.count + " entries")
              : (root.screen === "config" || vault.state === "error" ? "Setup required" : "Locked")
            foreground: root.foreground
            fontFamily: root.fontFamily
            iconOpacity: root.ready || root.screen === "fingerprint" ? 1.0 : 0.65
            iconComponent: Component {
              Text {
                text: root.ready ? "󰍁" : (root.screen === "fingerprint" ? "󰈷" : "󰌾")
                color: root.ready || root.screen === "fingerprint" ? root.foreground : root.dim
                font.family: root.fontFamily
                font.pixelSize: Style.font.display
              }
            }
            trailingControl: root.ready ? listHeroTrailing : fingerprintHeroTrailing
          }

          Item {
            id: statusSlot
            visible: vault.errorText !== "" || (!root.ready && root.lastAction !== "")
            width: parent.width
            height: statusRow.implicitHeight

            readonly property bool fingerprintRetry: root.screen === "fingerprint" && vault.state !== "authenticating"
            readonly property bool fingerprintError: root.screen === "fingerprint" && root.lastAction === "Try again"

            MouseArea {
              anchors.fill: parent
              enabled: statusSlot.fingerprintRetry
              hoverEnabled: statusSlot.fingerprintRetry
              cursorShape: statusSlot.fingerprintRetry ? Qt.PointingHandCursor : Qt.ArrowCursor
              onClicked: root.unlockWithFingerprint()
            }

            RowLayout {
              id: statusRow
              width: parent.width
              spacing: Style.space(8)

              Text {
                visible: root.screen === "fingerprint" && vault.errorText === ""
                text: "󰈷"
                color: statusSlot.fingerprintError ? root.urgent : root.dim
                font.family: root.fontFamily
                font.pixelSize: Style.font.title
                Layout.alignment: Qt.AlignVCenter
              }

              Text {
                id: statusText
                Layout.fillWidth: true
                text: vault.errorText !== "" ? vault.errorText : root.lastAction
                color: vault.errorText !== "" || statusSlot.fingerprintError ? root.urgent : root.dim
                font.family: root.fontFamily
                font.pixelSize: Style.font.bodySmall
                wrapMode: Text.WordWrap
              }
            }
          }

          Column {
            visible: !root.ready
            width: parent.width
            spacing: Style.space(8)

            Text {
              visible: root.screen === "config"
              width: parent.width
              text: "Database setup"
              color: root.foreground
              font.family: root.fontFamily
              font.pixelSize: Style.font.heading
            }

            RowLayout {
              visible: root.screen === "config"
              width: parent.width
              spacing: Style.space(8)

              TextField {
                id: setupPathField
                Layout.fillWidth: true
                text: root.setupPathText
                placeholderText: "Path to KeePassXC .kdbx file"
                foreground: root.foreground
                onTextChanged: root.setupPathText = text
                onAccepted: setupPasswordField.forceActiveFocus()
                Keys.onEscapePressed: root.close()
              }

              Button {
                id: browseButton
                text: "Browse"
                tooltipText: "Choose a KeePassXC database file"
                foreground: root.foreground
                fontFamily: root.fontFamily
                Layout.alignment: Qt.AlignVCenter
                onClicked: root.browseDatabase()
              }
            }

            TextField {
              id: setupPasswordField
              visible: root.screen === "config"
              width: parent.width
              text: root.setupPasswordText
              placeholderText: "Database password"
              password: true
              foreground: root.foreground
              onTextChanged: root.setupPasswordText = text
              onAccepted: root.unlock()
              Keys.onEscapePressed: root.close()
            }

            ActionButton {
              visible: root.screen === "config"
              width: parent.width
              title: "Unlock database"
              subtitle: "Typed once after the helper starts; later unlocks use fingerprint"
              iconText: "󰌋"
              onActivated: root.unlock()
            }
          }

          Column {
            visible: root.ready
            width: parent.width
            spacing: Style.space(8)

            TextField {
              id: searchField
              width: parent.width
              placeholderText: "Search entries…"
              text: root.searchText
              foreground: root.foreground
              hasCursor: false
              onTextChanged: {
                var changed = root.searchText !== text
                root.searchText = text
                if (changed) root.selectedIndex = 0
                root.rebuildEntries()
              }
              Keys.onPressed: function(event) {
                if (event.key === Qt.Key_Escape) {
                  root.close()
                  event.accepted = true
                } else if (event.key === Qt.Key_Down) {
                  root.select(1)
                  event.accepted = true
                } else if (event.key === Qt.Key_Up) {
                  root.select(-1)
                  event.accepted = true
                } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                  if (event.modifiers & Qt.ShiftModifier) root.copyDetail("username")
                  else root.readSelected()
                  event.accepted = true
                } else if ((event.modifiers & Qt.ControlModifier) && (event.key === Qt.Key_C)) {
                  root.readSelected()
                  event.accepted = true
                } else if ((event.modifiers & Qt.ControlModifier) && (event.key === Qt.Key_B)) {
                  root.copyDetail("username")
                  event.accepted = true
                } else if ((event.modifiers & Qt.ControlModifier) && (event.key === Qt.Key_G)) {
                  root.openGui()
                  event.accepted = true
                }
              }
            }

            ListView {
              id: resultList
              width: parent.width
              height: Math.min(contentHeight, Style.space(280))
              model: entriesModel
              clip: true
              spacing: Style.space(1)
              currentIndex: root.selectedIndex
              boundsBehavior: Flickable.StopAtBounds
              ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }

              delegate: CursorSurface {
                id: entryRow
                required property int index
                required property string entryId
                required property string entryTitle
                required property string entryUsername

                readonly property bool selected: entriesModel.count > 0 && root.selectedIndex === index

                width: ListView.view.width
                height: Style.space(44)
                foreground: root.foreground
                hasCursor: entryHover.containsMouse || entryRow.selected

                Rectangle {
                  anchors.fill: parent
                  radius: Style.space(4)
                  color: entryRow.selected
                    ? Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.18)
                    : "transparent"
                }

                Rectangle {
                  visible: entryRow.selected
                  width: Style.space(3)
                  height: parent.height
                  color: root.foreground
                }

                MouseArea {
                  id: entryHover
                  anchors.fill: parent
                  hoverEnabled: true
                  cursorShape: Qt.PointingHandCursor
                  onClicked: {
                    root.selectedIndex = entryRow.index
                  }
                }

                RowLayout {
                  anchors.fill: parent
                  anchors.leftMargin: Style.space(6)
                  anchors.rightMargin: Style.space(4)
                  spacing: Style.space(4)

                  ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0

                    Text {
                      Layout.fillWidth: true
                      text: entryRow.entryTitle
                      color: root.foreground
                      font.family: root.fontFamily
                      font.pixelSize: Style.font.body
                      elide: Text.ElideRight
                    }

                    Text {
                      Layout.fillWidth: true
                      text: entryRow.entryUsername
                      color: root.dim
                      font.family: root.fontFamily
                      font.pixelSize: Style.font.caption
                      elide: Text.ElideRight
                    }
                  }

                  CountdownRing {
                    visible: root.clipboardHot && entryRow.entryId === root.clipboardEntryId
                    Layout.alignment: Qt.AlignVCenter
                    Layout.preferredWidth: copyPasswordButton.implicitHeight
                    Layout.preferredHeight: copyPasswordButton.implicitHeight
                    implicitWidth: Style.space(20)
                    implicitHeight: Style.space(20)
                    fraction: root.clipboardTimeoutSec > 0
                      ? root.clipboardRemainingSeconds / root.clipboardTimeoutSec : 0
                    strokeColor: root.urgent
                    trackOpacity: 0.25
                    lineWidth: Math.max(2, width * 0.16)
                  }

                  Button {
                    id: copyPasswordButton
                    iconText: "󰌋"
                    tooltipText: "Copy password"
                    foreground: root.foreground
                    fontFamily: root.fontFamily
                    iconSize: Style.font.heading
                    horizontalPadding: Style.space(5)
                    verticalPadding: Style.space(2)
                    Layout.rightMargin: Style.space(4)
                    Layout.alignment: Qt.AlignVCenter
                    onClicked: {
                      root.selectedIndex = entryRow.index
                      root.readSelected()
                    }
                  }
                }
              }
            }

            Text {
              visible: entriesModel.count === 0 && root.searchText !== ""
              width: parent.width
              text: "No matching entries"
              color: root.dim
              font.family: root.fontFamily
              font.pixelSize: Style.font.body
              horizontalAlignment: Text.AlignHCenter
            }

            Text {
              visible: entriesModel.count === 0 && root.searchText === ""
              width: parent.width
              text: "No entries in this database"
              color: root.dim
              font.family: root.fontFamily
              font.pixelSize: Style.font.body
              horizontalAlignment: Text.AlignHCenter
            }

            Column {
              visible: root.selectedEntry
              width: parent.width
              spacing: Style.space(4)

              Rectangle {
                width: parent.width
                height: 1
                color: Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.2)
              }

              Item {
                width: parent.width
                height: Style.space(8)
              }

              DetailRow {
                width: parent.width
                label: "Username"
                value: root.detailUsername
                onCopyRequested: root.copyDetail("username")
              }

              DetailRow {
                width: parent.width
                label: "Password"
                value: root.detailPassword
                secret: true
                revealed: root.passwordRevealed
                onCopyRequested: root.copyDetail("password")
                onRevealRequested: root.passwordRevealed = !root.passwordRevealed
              }

              DetailRow {
                width: parent.width
                label: "URL"
                value: root.detailUrl
                showBrowser: true
                onCopyRequested: root.copyDetail("url")
                onOpenRequested: root.openSelectedUrl()
              }

              DetailRow {
                width: parent.width
                label: "Notes"
                value: root.detailNotes
                wrapValue: true
                onCopyRequested: root.copyDetail("notes")
              }
            }
          }
        }
      }
    }
  }

  ListModel { id: entriesModel }

  component GearButton: Button {
    iconText: "󰒓"
    tooltipText: "Choose the database file or unlock with a password"
    foreground: root.foreground
    fontFamily: root.fontFamily
    iconSize: Style.font.subtitle * 1.5
    horizontalPadding: Style.space(5)
    verticalPadding: Style.space(2)
    onClicked: root.openDatabaseSettings()
  }

  property Component fingerprintHeroTrailing: Component {
    GearButton {
      visible: root.screen === "fingerprint"
    }
  }

  property Component listHeroTrailing: Component {
    RowLayout {
      spacing: Style.space(8)

      Button {
        iconText: "󰖟"
        tooltipText: "Open KeePassXC"
        foreground: root.foreground
        fontFamily: root.fontFamily
        iconSize: Style.font.subtitle * 1.5
        horizontalPadding: Style.space(5)
        verticalPadding: Style.space(2)
        Layout.alignment: Qt.AlignVCenter
        onClicked: root.openGui()
      }

      Item {
        implicitWidth: lockButton.implicitWidth + Style.space(8)
        implicitHeight: lockButton.implicitHeight + Style.space(8)
        Layout.alignment: Qt.AlignVCenter

        Button {
          id: lockButton
          anchors.centerIn: parent
          iconText: "󰌾"
          tooltipText: "Lock database"
          foreground: root.foreground
          fontFamily: root.fontFamily
          iconSize: Style.font.subtitle * 1.5
          horizontalPadding: Style.space(5)
          verticalPadding: Style.space(2)
          onClicked: vault.lock()
        }

        CountdownRing {
          anchors.fill: parent
          enabled: false
          visible: root.ready && vault.idleTimeoutSec > 0 && vault.idleRemainingSeconds > 0
          fraction: vault.idleTimeoutSec > 0
            ? vault.idleRemainingSeconds / vault.idleTimeoutSec : 0
          strokeColor: root.foreground
          trackOpacity: 0.2
          lineWidth: 1
          radiusInset: 2
        }
      }

      GearButton {
        Layout.alignment: Qt.AlignVCenter
      }
    }
  }

  component CountdownRing: Item {
    id: countdown
    property real fraction: 0
    property color strokeColor: root.foreground
    property real trackOpacity: 0.2
    property real lineWidth: 1
    property real radiusInset: 0

    onFractionChanged: ring.requestPaint()
    onStrokeColorChanged: ring.requestPaint()
    onWidthChanged: ring.requestPaint()
    onHeightChanged: ring.requestPaint()

    Canvas {
      id: ring
      anchors.fill: parent

      function paintColor(c, a) {
        return "rgba(" + Math.round(c.r * 255) + "," + Math.round(c.g * 255) + "," + Math.round(c.b * 255) + "," + a + ")"
      }

      onPaint: {
        var ctx = getContext("2d")
        ctx.reset()
        var cx = width / 2
        var cy = height / 2
        var line = countdown.lineWidth
        var inset = countdown.radiusInset > 0 ? countdown.radiusInset : line
        var radius = Math.min(width, height) / 2 - inset
        var frac = Math.max(0, Math.min(1, countdown.fraction))

        ctx.lineWidth = line
        ctx.lineCap = "round"

        ctx.strokeStyle = paintColor(root.foreground, countdown.trackOpacity)
        ctx.beginPath()
        ctx.arc(cx, cy, radius, 0, Math.PI * 2)
        ctx.stroke()

        if (frac > 0) {
          ctx.strokeStyle = paintColor(countdown.strokeColor, 1)
          ctx.beginPath()
          ctx.arc(cx, cy, radius, -Math.PI / 2, -Math.PI / 2 + frac * Math.PI * 2, false)
          ctx.stroke()
        }
      }
    }
  }

  component DetailRow: Item {
    id: detailRow
    property string label: ""
    property string value: ""
    property bool wrapValue: false
    property bool showBrowser: false
    property bool secret: false
    property bool revealed: false
    signal copyRequested()
    signal openRequested()
    signal revealRequested()

    readonly property string displayValue: {
      if (detailRow.value === "") return "—"
      if (detailRow.secret && !detailRow.revealed) return "••••••••"
      return detailRow.value
    }

    implicitHeight: detailLayout.implicitHeight + Style.space(4)
    implicitWidth: detailLayout.implicitWidth

    RowLayout {
      id: detailLayout
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.verticalCenter: parent.verticalCenter
      anchors.leftMargin: Style.space(6)
      anchors.rightMargin: Style.space(8)
      spacing: Style.space(8)

      Text {
        text: detailRow.label
        color: root.dim
        font.family: root.fontFamily
        font.pixelSize: Style.font.caption
        Layout.preferredWidth: Style.space(72)
        Layout.alignment: Qt.AlignVCenter
      }

      Text {
        Layout.fillWidth: true
        text: detailRow.displayValue
        color: detailRow.value !== "" ? root.foreground : root.dim
        font.family: root.fontFamily
        font.pixelSize: Style.font.bodySmall
        wrapMode: detailRow.wrapValue ? Text.WordWrap : Text.NoWrap
        elide: detailRow.wrapValue ? Text.ElideNone : Text.ElideRight
        maximumLineCount: detailRow.wrapValue ? 4 : 1
        Layout.alignment: Qt.AlignVCenter
      }

      Button {
        visible: detailRow.secret
        iconText: detailRow.revealed ? "󰈉" : "󰈈"
        tooltipText: detailRow.revealed ? "Hide password" : "Show password"
        foreground: root.foreground
        fontFamily: root.fontFamily
        iconSize: Style.font.heading
        horizontalPadding: Style.space(5)
        verticalPadding: Style.space(2)
        Layout.alignment: Qt.AlignVCenter
        enabled: detailRow.value !== ""
        onClicked: detailRow.revealRequested()
      }

      Button {
        visible: detailRow.showBrowser
        iconText: "󰏌"
        tooltipText: "Open in browser"
        foreground: root.foreground
        fontFamily: root.fontFamily
        iconSize: Style.font.heading
        horizontalPadding: Style.space(5)
        verticalPadding: Style.space(2)
        Layout.alignment: Qt.AlignVCenter
        enabled: detailRow.value !== ""
        onClicked: detailRow.openRequested()
      }

      Button {
        iconText: "󰆏"
        tooltipText: "Copy " + detailRow.label.toLowerCase()
        foreground: root.foreground
        fontFamily: root.fontFamily
        iconSize: Style.font.heading
        horizontalPadding: Style.space(5)
        verticalPadding: Style.space(2)
        Layout.alignment: Qt.AlignVCenter
        enabled: detailRow.value !== ""
        onClicked: detailRow.copyRequested()
      }
    }
  }

  SequentialAnimation {
    id: errorBlink
    PropertyAnimation {
      target: statusText
      property: "opacity"
      to: 0.15
      duration: 140
    }
    PropertyAnimation {
      target: statusText
      property: "opacity"
      to: 1
      duration: 140
    }
  }

  Timer {
    id: clipboardTimer
    interval: 100
    repeat: true
    running: false
    onTriggered: {
      root.clipboardRemainingSeconds = Math.max(0, (root.clipboardDeadlineMs - Date.now()) / 1000)
      if (root.clipboardRemainingSeconds <= 0) {
        running = false
        clipboardCheck.running = false
        clipboardCheck.running = true
      }
    }
  }

  Process {
    id: clipboardCheck
    command: ["/usr/bin/wl-paste", "--no-newline"]
    stdout: StdioCollector {
      id: clipboardCheckStdout
      waitForEnd: true
    }
    onExited: function(exitCode) {
      if (exitCode !== 0) root.finishClipboardCheck(null)
      else root.finishClipboardCheck(clipboardCheckStdout.text)
    }
  }

  component ActionButton: CursorSurface {
    id: actionButton
    property string title: ""
    property string subtitle: ""
    property string iconText: "󰌋"
    signal activated()

    hasCursor: actionHover.containsMouse
    foreground: root.foreground
    implicitHeight: actionRow.implicitHeight + Style.spacing.rowPaddingX

    MouseArea {
      id: actionHover
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onClicked: actionButton.activated()
    }

    RowLayout {
      id: actionRow
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.verticalCenter: parent.verticalCenter
      anchors.leftMargin: Style.space(10)
      anchors.rightMargin: Style.space(10)
      spacing: Style.space(8)

      Text {
        text: actionButton.iconText
        color: root.foreground
        font.family: root.fontFamily
        font.pixelSize: Style.font.heading
        Layout.alignment: Qt.AlignVCenter
      }

      ColumnLayout {
        Layout.fillWidth: true
        spacing: Style.space(1)

        Text {
          Layout.fillWidth: true
          text: actionButton.title
          color: root.foreground
          font.family: root.fontFamily
          font.pixelSize: Style.font.body
          elide: Text.ElideRight
        }

        Text {
          Layout.fillWidth: true
          text: actionButton.subtitle
          color: root.dim
          font.family: root.fontFamily
          font.pixelSize: Style.font.caption
          elide: Text.ElideRight
        }
      }

      Text {
        text: "󰅂"
        color: root.foreground
        font.family: root.fontFamily
        font.pixelSize: Style.font.heading
        Layout.alignment: Qt.AlignVCenter
      }
    }
  }
}
