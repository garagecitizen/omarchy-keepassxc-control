import QtQuick
import Quickshell
import Quickshell.Io
import "Model.js" as Model

Item {
  id: root

  property var settings: ({})
  property string databasePath: String(Model.setting(settings, "databasePath", "") || "")
  property int idleTimeoutSec: Model.intSetting(settings, "idleTimeoutSec", 180, 30, 3600)

  property bool unlocked: false
  property bool hasStoredSecret: false
  property bool fingerprintAvailable: false
  property bool openFailed: false
  property string state: "starting"
  property string errorText: ""
  property var entries: []

  property int nextRequestId: 1
  property var pendingRequests: ({})
  property string queuedUnlockPath: ""
  property string queuedUnlockPassword: ""
  property bool unlocking: false
  property int helperFailures: 0
  property bool helperUnavailable: false
  property real idleRemainingSeconds: 0
  property real idleDeadlineMs: 0

  signal fieldRead(string value, string entryId)
  signal detailsRead(string entryId, string username, string password, string url, string notes)
  signal clipboardWritten(string entryId)
  signal helperFailed(string message)
  signal authProgress(string message)
  signal filePicked(string path)
  signal filePickCancelled()
  signal filePickFailed(string message)
  signal idleLocked()

  function reportFailure(message) {
    root.errorText = String(message || "KeePassXC operation failed")
    root.helperFailed(root.errorText)
  }

  function projectDirectory() {
    var url = String(Qt.resolvedUrl("."))
    return decodeURIComponent(url.replace(/^file:\/\//, "")).replace(/\/$/, "")
  }

  function helperBinary() {
    return root.projectDirectory() + "/keepassxc-control-helper"
  }

  function start() {
    if (root.helperUnavailable || helperProcess.running || helperExistsCheck.running)
      return
    if (helperRestart.running || root.helperFailures >= 5) return
    helperExistsCheck.command = ["test", "-f", root.helperBinary()]
    helperExistsCheck.running = true
  }

  function refresh() {
    if (root.state === "authenticating" || root.unlocking) return
    if (!helperProcess.running) {
      start()
      return
    }
    send("status", {})
  }

  function configure(path) {
    databasePath = String(path || "")
    send("configure", {
      path: databasePath,
      idle_timeout_seconds: idleTimeoutSec
    })
  }

  function unlock(path, password) {
    var requestedPath = String(path || databasePath || "")
    var requestedPassword = String(password || "")
    if (!helperProcess.running) {
      queuedUnlockPath = requestedPath
      queuedUnlockPassword = requestedPassword
      start()
      return
    }
    root.unlocking = true
    databasePath = requestedPath
    var payload = { path: databasePath, idle_timeout_seconds: idleTimeoutSec }
    if (requestedPassword !== "") payload.password = requestedPassword
    else {
      root.state = "authenticating"
      root.errorText = ""
    }
    send("unlock", payload)
  }

  function list() {
    send("list", {})
  }

  function readField(entryId) {
    var id = String(entryId || "")
    send("read_field", { entry_id: id }, { entryId: id })
  }

  function readDetails(entryId) {
    send("read_details", {
      entry_id: String(entryId || "")
    })
  }

  function writeClipboard(text, entryId) {
    send("write_clipboard", { text: String(text || "") }, { entryId: String(entryId || "") })
  }

  function applyIdleRemaining(seconds) {
    var remaining = Math.max(0, Number(seconds || 0))
    root.idleDeadlineMs = remaining > 0 ? Date.now() + remaining * 1000 : 0
    root.idleRemainingSeconds = remaining
  }

  function applyLocked(nextState) {
    root.unlocked = false
    root.entries = []
    root.state = nextState
  }

  function applyIdleLock() {
    root.applyIdleRemaining(0)
    root.idleLocked()
    root.applyLocked(root.databasePath === "" ? "setup" : "locked")
  }

  function lock() {
    send("lock", {})
  }

  function pickDatabaseFile() {
    if (fileSelect.running) return
    fileSelect.running = true
  }

  function send(operation, payload, extra) {
    if (!helperProcess.running) return

    var request = { id: nextRequestId++, op: operation }
    var body = payload || {}
    for (var key in body) request[key] = body[key]
    var pending = { op: operation }
    if (extra && extra.entryId) pending.entryId = extra.entryId
    pendingRequests[String(request.id)] = pending
    helperProcess.write(JSON.stringify(request) + "\n")
  }

  function handleLine(line) {
    var response
    try {
      response = JSON.parse(String(line || ""))
    } catch (error) {
      root.state = "error"
      root.reportFailure("The KeePassXC helper returned invalid data")
      return
    }

    root.helperFailures = 0

    if (response.progress) {
      root.errorText = ""
      root.authProgress(String(response.message || ""))
      return
    }

    var requestId = String(response.id)
    var request = pendingRequests[requestId] || {}
    delete pendingRequests[requestId]

    if (!response.ok) {
      if (request.op === "unlock") root.unlocking = false
      var code = response.error && response.error.code ? String(response.error.code) : ""
      if (request.op === "unlock" || request.op === "list") {
        if (code === "auth_failed") {
          root.applyLocked(root.databasePath === "" ? "setup" : "locked")
          root.errorText = ""
          return
        }
        if (code === "locked") {
          if (root.unlocked || root.state === "unlocked") {
            root.applyIdleLock()
            return
          }
          root.applyLocked(root.databasePath === "" ? "setup" : "locked")
        } else {
          root.openFailed = true
          root.applyLocked("setup")
        }
      } else if (request.op === "configure") {
        root.applyLocked("setup")
      } else if (code === "locked") {
        if (root.unlocked || root.state === "unlocked") {
          root.applyIdleLock()
          return
        }
        root.applyLocked("locked")
      }
      root.reportFailure(response.error && response.error.message
        ? String(response.error.message) : "KeePassXC operation failed")
      return
    }

    var result = response.result || {}
    if (request.op === "unlock") {
      root.unlocking = false
      root.errorText = ""
      root.openFailed = false
      if (result.secret_stored !== undefined)
        root.hasStoredSecret = Boolean(result.secret_stored)
      root.list()
    } else if (request.op === "list") {
      var entries = Array.isArray(result.entries) ? result.entries : []
      root.errorText = ""
      root.openFailed = false
      root.entries = entries
      root.unlocked = true
      root.state = "unlocked"
      root.applyIdleRemaining(root.idleTimeoutSec)
    } else if (request.op === "read_field") {
      root.fieldRead(String(result.value || ""), String(request.entryId || ""))
    } else if (request.op === "read_details") {
      root.detailsRead(
        String(result.entry_id || ""),
        String(result.username || ""),
        String(result.password || ""),
        String(result.url || ""),
        String(result.notes || "")
      )
    } else if (request.op === "write_clipboard") {
      root.clipboardWritten(String(request.entryId || ""))
    } else if (request.op === "lock") {
      root.applyIdleRemaining(0)
      root.applyLocked("locked")
    } else if (request.op === "status") {
      if (result.path) root.databasePath = String(result.path)
      root.hasStoredSecret = Boolean(result.has_stored_secret)
      root.fingerprintAvailable = Boolean(result.fingerprint_available)
      if (root.state === "authenticating")
        return
      if (String(result.state || "") === "unlocked") {
        root.applyIdleRemaining(result.idle_remaining_seconds)
        if (!(root.unlocked && root.state === "unlocked"))
          root.list()
      } else if (root.state === "unlocked") {
        root.applyIdleLock()
      } else if (root.databasePath === "") {
        root.applyLocked("setup")
      }
    }
  }

  Process {
    id: fileSelect
    command: [
      "omarchy", "file", "select",
      "--title", "Select KeePassXC database",
      "--extensions", "kdbx"
    ]
    stdout: StdioCollector {
      id: fileSelectStdout
      waitForEnd: true
    }
    onExited: function(exitCode) {
      var picked = String(fileSelectStdout.text || "").trim().split("\n")[0]
      if (exitCode === 0 && picked !== "") {
        root.filePicked(picked)
        return
      }
      if (exitCode === 1 || (exitCode === 0 && picked === "")) {
        root.filePickCancelled()
        return
      }
      root.filePickFailed("Could not open the file picker")
    }
  }

  Process {
    id: helperExistsCheck
    running: false
    onExited: function(exitCode) {
      if (exitCode !== 0) {
        root.helperUnavailable = true
        root.pendingRequests = ({})
        root.state = "error"
        root.reportFailure("The KeePassXC helper is not built. Run make.")
        return
      }
      if (!helperProcess.running) helperProcess.running = true
    }
  }

  Process {
    id: helperProcess
    command: [root.helperBinary()]
    running: false
    stdinEnabled: true

    stdout: SplitParser {
      onRead: function(data) { root.handleLine(data) }
    }
    stderr: StdioCollector {
      id: helperStderr
      waitForEnd: true
    }

    onStarted: {
      root.state = root.databasePath === "" ? "setup" : "locked"
      if (queuedUnlockPath !== "") {
        var path = queuedUnlockPath
        var password = queuedUnlockPassword
        queuedUnlockPath = ""
        queuedUnlockPassword = ""
        root.unlock(path, password)
        return
      }
      if (root.databasePath !== "") root.configure(root.databasePath)
      root.refresh()
    }
    onExited: function(exitCode) {
      root.unlocking = false
      root.pendingRequests = ({})
      root.applyIdleRemaining(0)
      root.applyLocked("error")
      var stderr = String(helperStderr.text || "").trim()
      if (exitCode !== 0) {
        if (stderr !== "")
          root.reportFailure(stderr)
        else if (root.errorText === "")
          root.reportFailure("The KeePassXC helper stopped")
        else
          root.helperFailed(root.errorText)
      }
      root.helperFailures += 1
      if (root.helperFailures >= 5) return
      helperRestart.interval = Math.min(16000, 1000 * Math.pow(2, root.helperFailures - 1))
      helperRestart.restart()
    }
  }

  Timer {
    id: helperRestart
    interval: 1000
    repeat: false
    onTriggered: root.start()
  }

  Timer {
    interval: 1000
    running: root.unlocked
    repeat: true
    triggeredOnStart: true
    onTriggered: root.refresh()
  }

  Timer {
    interval: 100
    running: root.state === "unlocked" && root.idleRemainingSeconds > 0
    repeat: true
    onTriggered: {
      root.idleRemainingSeconds = Math.max(0, (root.idleDeadlineMs - Date.now()) / 1000)
    }
  }

  Component.onCompleted: root.start()
}
