function shouldPersistDatabasePath(setting, nextPath, options) {
  var unlocking = !!(options && options.unlocking)
  var source = options && options.source ? String(options.source) : ""
  if (unlocking || source === "status") return false
  var next = String(nextPath || "")
  if (next === "") return false
  return String(setting || "") !== next
}

function visibleScreen(state) {
  var current = state || {}
  var path = String(current.path || "")
  if (!!current.unlocked) return "list"
  if (
    current.openFailed ||
    current.preferPassword ||
    path === "" ||
    !current.hasStoredSecret ||
    current.fingerprintAvailable === false
  ) {
    return "config"
  }
  return "fingerprint"
}

function setting(settings, name, fallback) {
  var value = settings ? settings[name] : undefined
  return value === undefined || value === null ? fallback : value
}

function intSetting(settings, name, fallback, min, max) {
  var value = parseInt(String(setting(settings, name, fallback)), 10)
  if (!isFinite(value)) value = fallback
  return Math.max(min, Math.min(max, value))
}

function nextSelectedIndex(current, count) {
  if (count === 0) return current
  return Math.max(0, Math.min(current, count - 1))
}

function filterEntries(entries, query) {
  var normalizedQuery = String(query || "").trim().toLowerCase()
  var source = Array.isArray(entries) ? entries : []
  if (normalizedQuery === "") return source.slice()

  return source.filter(function(entry) {
    var title = String(entry && entry.title || "").toLowerCase()
    return title.indexOf(normalizedQuery) !== -1
  })
}

function safeEntries(entries) {
  var source = Array.isArray(entries) ? entries : []
  return source.filter(function(entry) {
    return entry && String(entry.id || "") !== ""
  }).map(function(entry) {
    return {
      id: String(entry.id),
      title: String(entry.title || "(untitled)"),
      username: String(entry.username || "")
    }
  }).sort(function(left, right) {
    var byTitle = left.title.toLowerCase().localeCompare(right.title.toLowerCase())
    if (byTitle !== 0) return byTitle
    return left.username.toLowerCase().localeCompare(right.username.toLowerCase())
  })
}

if (typeof module !== "undefined") {
  module.exports = {
    filterEntries: filterEntries,
    safeEntries: safeEntries,
    shouldPersistDatabasePath: shouldPersistDatabasePath,
    visibleScreen: visibleScreen,
    nextSelectedIndex: nextSelectedIndex,
    setting: setting,
    intSetting: intSetting
  }
}
