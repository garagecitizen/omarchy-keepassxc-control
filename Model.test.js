const assert = require("assert")
const {
  shouldPersistDatabasePath,
  visibleScreen,
  nextSelectedIndex,
  setting,
  intSetting,
} = require("./Model.js")

assert.strictEqual(
  shouldPersistDatabasePath("", "/home/mike/db.kdbx", { unlocking: true }),
  false,
  "do not persist while unlock is in flight"
)
assert.strictEqual(
  shouldPersistDatabasePath("/home/mike/db.kdbx", "/tmp/real/db.kdbx", { source: "status" }),
  false,
  "do not persist a helper-resolved path from status"
)
assert.strictEqual(
  shouldPersistDatabasePath("/home/mike/db.kdbx", "/home/mike/db.kdbx", { source: "unlock" }),
  false,
  "do not persist an unchanged path"
)
assert.strictEqual(
  shouldPersistDatabasePath("", "/home/mike/db.kdbx", { source: "unlock" }),
  true,
  "persist the typed path after a successful unlock"
)

assert.strictEqual(
  visibleScreen({
    path: "/home/mike/db.kdbx",
    hasStoredSecret: true,
    unlocked: true,
    entryCount: 36,
    preferPassword: false,
    openFailed: false,
  }),
  "list",
  "typed unlock with entries must leave config"
)
assert.strictEqual(
  visibleScreen({
    path: "/home/mike/db.kdbx",
    hasStoredSecret: false,
    unlocked: false,
    entryCount: 0,
    preferPassword: true,
    openFailed: false,
  }),
  "config"
)
assert.strictEqual(
  visibleScreen({
    path: "/home/mike/db.kdbx",
    hasStoredSecret: true,
    unlocked: false,
    preferPassword: false,
    openFailed: false,
  }),
  "fingerprint",
  "path plus secret plus locked is fingerprint"
)
assert.strictEqual(
  visibleScreen({
    path: "/home/mike/db.kdbx",
    hasStoredSecret: true,
    unlocked: true,
    preferPassword: false,
    openFailed: false,
  }),
  "list",
  "unlocked with zero entries is still list"
)
assert.strictEqual(
  visibleScreen({
    path: "/home/mike/db.kdbx",
    hasStoredSecret: true,
    unlocked: false,
    preferPassword: false,
    openFailed: true,
  }),
  "config",
  "openFailed with a retained secret stays on config"
)
assert.strictEqual(
  visibleScreen({
    path: "/home/mike/db.kdbx",
    hasStoredSecret: true,
    unlocked: false,
    preferPassword: true,
    openFailed: false,
  }),
  "config",
  "gear preferPassword is config"
)
assert.strictEqual(
  visibleScreen({
    path: "/home/mike/db.kdbx",
    hasStoredSecret: true,
    unlocked: false,
    preferPassword: false,
    openFailed: false,
    fingerprintAvailable: false,
  }),
  "config",
  "no fingerprint hardware is config"
)
assert.strictEqual(
  nextSelectedIndex(5, 0),
  5,
  "empty rebuild keeps the requested index"
)
assert.strictEqual(
  nextSelectedIndex(5, 3),
  2,
  "later non-empty enumeration clamps the saved index"
)
assert.strictEqual(
  nextSelectedIndex(1, 10),
  1,
  "in-range selection survives a non-empty rebuild"
)
assert.strictEqual(
  nextSelectedIndex(0, 0),
  0,
  "path-change reset to zero stays zero across an empty rebuild"
)

assert.strictEqual(setting({ databasePath: "/tmp/db.kdbx" }, "databasePath", ""), "/tmp/db.kdbx")
assert.strictEqual(intSetting({ idleTimeoutSec: "90" }, "idleTimeoutSec", 180, 30, 3600), 90)

console.log("Model.test.js ok")
