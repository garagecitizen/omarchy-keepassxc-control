# KeePassXC Control — product spec

This file is the product contract. Implementation follows it. If code and
this file disagree, this file wins. Do not add behavior that is not written
here.

[`AGENTS.md`](AGENTS.md) is layout, protocol, and commands for agents. It
is not a second product spec.

---

## What this is

One Omarchy QML popup for **one** KeePassXC `.kdbx` file, plus a bar icon.

The popup can show exactly one of these:

| Screen | When |
| --- | --- |
| **Config** | Gear; or no usable database path / stored secret; or fingerprint verification is not available; or the database failed to open |
| **Fingerprint** | Database is locked, a secret is stored for that path, fingerprint verification is available, and Config was not just opened via gear |
| **List** | Database is unlocked |

KeePassXC itself remains the editor. Groups as a UI, editing, deletion,
multiple open databases, and TOTP are out of scope. List still flattens
every entry in the file into one sorted row list.

---

## Stored facts

Two facts matter. They are independent.

| Fact | Meaning |
| --- | --- |
| **Path** | Configured `.kdbx` location (`databasePath` on the plugin’s `shell.json` entry) |
| **Secret** | KeePassXC database password retained in the helper process for that path |

Settings live inline on the `keepassxc.control` bar entry in
`~/.config/omarchy/shell.json`. There is no separate plugin settings file.

| Field | Meaning | Default | Clamp |
| --- | --- | --- | --- |
| `databasePath` | Path to the `.kdbx` file | empty | — |
| `idleTimeoutSec` | Idle lock, seconds | 180 | 30–3600 |
| `clipboardTimeoutSec` | Clipboard clear, seconds | 30 | 5–120 |

Missing or invalid fields use those defaults, then the clamp. The Config
screen is the only **popup** place to set the path. A successful typed
unlock persists `databasePath` into the entry. Idle and clipboard timeouts
have no popup control; they are changed in `shell.json` or with
`omarchy bar set`.

After a successful **typed** unlock, the helper retains the password in
memory for that path. Idle lock and explicit lock wipe the decrypted vault
and entry cache only. They do **not** drop the retained password.

The secret dies with the helper (shell restart, crash, `make reload`,
logout). “No stored secret” means this helper has no retained password:
fresh process, path change, or never typed. If the secret is missing, the
user types the database password on Config.

The password is not written to the OS keyring. There is no third
“remembered typed password” in the popup.

---

## Bar

The bar icon toggles the popup. Right-click locks the in-memory vault.
Middle-click opens KeePassXC. While a copy is still on the clipboard
countdown, the icon shows a hot marker.

---

## 1. Config

Show Config when any of these is true:

1. The user opens **gear**.
2. There is **no database path**, or **no stored secret** for the current path.
3. Fingerprint verification is **not available** (`fprintd-verify` missing).
4. Opening the database **failed** (missing file, rejected password, rejected
   stored secret, unreadable file, helper could not list entries).

A failure in (4) stays on Config across popup close until the next
successful unlock or the configured path changes. Gear-Config does **not**
stick across popup close: the next open follows path/secret/fingerprint
again.

Config is the only popup place to set the path and type the database
password. Header meta is “Setup required”.

Config contains:

- database path field
- Browse (`omarchy file select` for `kdbx`)
- database password field
- Unlock

Browse closes the popup, runs the file picker, then reopens Config with
the chosen path (or the previous Config state if the picker is cancelled
or fails). It does not persist the path until typed unlock succeeds.

Gear is available from Fingerprint and from List. Gear is not shown on
Config (the user is already there). Gear from List locks the in-memory
vault first, then shows Config. Gear does **not** delete the stored
secret. It is the typed-password escape hatch.

If a fingerprint verify is in flight, gear **cancels** that wait (same
sensor outcome as lock: `auth_failed`, secret kept). Config stays on
screen until typed unlock succeeds or the popup is closed. A late
fingerprint match must not jump to List.

Unlock with an empty path or password stays on Config and says both are
required.

---

## 2. Typed unlock

On Config, Unlock (or Enter in the password field) with a non-empty path and
password:

1. Open that `.kdbx` with that password.
2. If it opens: retain the password in the helper for that path, persist
   the path if it changed, go to **List**.
3. If it fails: stay on **Config** and show the failure. Do not retain a
   rejected password.

Do not persist the path, reload the widget, or start fingerprint unlock
until this open has succeeded.

---

## 3. Fingerprint

Show Fingerprint when **all** of these are true:

- a database path is configured
- a secret is retained in this helper for that path
- fingerprint verification is available
- the in-memory vault is locked
- Config was not just opened via gear (gear stays on Config until typed
  unlock or the popup is closed)

Fingerprint contains:

- the locked-screen label at the top (title KeePassXC, meta Locked)
- gear, which opens Config
- one status line: the finger prompt. That line is also progress and
  error (“Try again”, mismatch, cancelled sensor, deadline).

Fingerprint does not contain a password field or a retry button.

Fingerprint offers unlock with `fprintd-verify`, then reopens the database
with the retained password. The popup starts one verify when Fingerprint is
shown. A mismatch, cancelled sensor, or verify deadline (45 seconds) stays
on Fingerprint and replaces the prompt with the error. Further verifies
start when the user taps that status line, or when Fingerprint is shown
again (popup closed and reopened). Do not use a timer loop to retry.

Lock (List control, bar right-click, or IPC) during an in-flight verify
cancels the sensor wait. Closing the popup during verify sends that same
lock, then clears details. The vault stays locked, the retained secret
stays, and the next open is **Fingerprint** unless path/secret/fingerprint
availability says otherwise. Helper `quit` also cancels, then the helper
exits. Cancelled verify is the same outcome as a cancelled sensor:
`auth_failed`, allow retry.

- **Match and open succeeds:** go to **List**.
- **No match / user cancels sensor / deadline:** stay on **Fingerprint**,
  show the error on the status line, allow retry. Do not jump to Config.
- **Open fails after a match** (file gone, stored secret rejected,
  fingerprint unavailable): go to **Config**. Path is kept when it is
  still known.

Fingerprint is not Config. Gear on Fingerprint opens Config.

---

## 4. List

A successful unlock — typed or fingerprint — shows List, including a
database with zero entries. Header meta is the entry count, or the last
copy status while the clipboard is hot.

List:

- enumerates every entry once after that unlock (all groups, flat)
- sorts by title, then username; missing titles show as “(untitled)”
- filters locally by title as the user types (not username)
- changing the filter selects the first match
- shows a linear list of titles and usernames
- copies password, username, URL, or notes without closing the popup
- shows details for the selected entry (username, password, URL, notes)
- can reveal or hide the password in details
- can open the selected URL in the browser if it is `http` or `https`
  (a scheme-less value is tried as `https://…`; anything else is refused)
- has a control to open KeePassXC
- has a lock control that locks the in-memory vault only
- has gear, which opens Config
- Escape closes the popup

Keyboard on List: Enter copies password; Shift+Enter or `b` copies
username; `g` opens KeePassXC.

After the lock control, the popup stays open and shows **Fingerprint** if
a secret is still retained and fingerprint verification is available,
otherwise **Config**.

Idle timeout is `idleTimeoutSec` without helper use (list, read, copy,
unlock). Typing in the local filter does not count as use. An open popup
does not pause that timer. When it fires, lock the vault (wipe the
decrypted vault, keep the retained password) and close the popup, even if
List is on screen. While unlocked, the List lock control shows remaining
idle time as a thin ring in the icon color.

A copy starts a `clipboardTimeoutSec` countdown. The copied row shows a
countdown ring. When the timer fires, clear the clipboard only if it still
holds the copied value. Copied username, URL, and notes use the same
timer as passwords.

---

## 5. List state

List **UI state** is:

- the search/filter text
- which entry is selected

That state **survives** lock and later unlock of the **same** database
(idle lock, lock control, close and reopen of the popup).

That state is **discarded** when the configured database path changes.
After a different database is unlocked, List is a fresh filter and
selection on the new enumeration.

The helper’s decrypted vault and entry cache are not list UI state. Lock
drops those. The next unlock enumerates again, then reapplies the saved
filter and selection if the path did not change. Details are loaded again
for the selected row; they do not survive lock or popup close.

---

## Transitions (normative)

```
no path or no secret          → Config
fingerprint unavailable       → Config
gear                          → Config (cancel in-flight verify; from List, lock vault first)
open failed                   → Config
browse                        → close popup, file select, reopen Config

typed password accepted       → retain secret, persist path if changed, List
typed password rejected       → Config (rejected password not retained)

path + secret + locked
  + fingerprint available
  and not on gear Config      → Fingerprint
fingerprint open succeeded    → List
fingerprint mismatch/deadline → Fingerprint
lock or close during verify   → cancel verify, Fingerprint
quit during verify            → cancel verify, helper exits
fingerprint open failed       → Config

lock (secret remains,
  fingerprint available)      → Fingerprint
lock (no secret, or no
  fingerprint)                → Config
idle                          → lock and close popup

path changed                  → drop list UI state
same path unlock              → restore list UI state
```

---

## Already decided (do not reopen unless the spec is edited)

Still in force:

- One popup, one database.
- Long-lived helper; QML does not keep the database password except to send
  a typed unlock or to display/copy a field the user asked for.
- The database password is not written to Secret Service. Leftover
  `application=keepassxc-control` items are deleted on helper start.
- Helper never puts the database password on argv.
- Browse uses `omarchy file select`, not an in-process file dialog.
- Fingerprint gates this helper’s use of the retained password. It does
  not survive process death.
- The helper stdin/stdout pipe is trusted as parent-private, not as
  authenticated IPC.
- The helper binary is executed from the writable plugin directory.
- Panel `IpcHandler` is open/close/show/hide/toggle/lock/status only.
  `show`/`hide` are aliases for open/close. No password read or unlock.

---

## Out of scope

- Multiple databases at once
- Editing, deleting, groups as a UI, TOTP
- Deleting the session secret on lock
- Treating lock as “go to Config”
- Inventing extra screens or extra unlock methods
- Timeout controls on the Config screen
