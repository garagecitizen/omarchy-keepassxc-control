# Agent notes

[`SPEC.md`](SPEC.md) is the product contract. If code and SPEC disagree,
SPEC wins. Do not add behavior that is not written there.

## Layout

- `helper/` — Rust crate. `vault.rs` opens the KDBX; `helper.rs` is the
  JSON-lines lifecycle; `credentials.rs` is `fprintd-verify` plus leftover
  Secret Service scrub; `clipboard.rs` writes `wl-copy` stdin.
- `Panel.qml`, `Service.qml`, `Model.js`, `manifest.json` — Omarchy bar
  plugin. `Service.qml` runs `./keepassxc-control-helper` from this
  directory (`make` copies the release binary there).

## Protocol

Newline-delimited JSON on stdin/stdout. Every request has `id` and `op`;
every response echoes the id. Ops: `configure`, `unlock`, `list`,
`read_field`, `read_details`, `write_clipboard`, `lock`, `status`, `quit`.
`list` returns id, title, username only. Secrets only on an explicit
`read_field` or `read_details`. The pipe is trusted as parent-private, not
as authenticated IPC.

## Constraints

- Database password never on argv, never in Secret Service. On helper
  start, leftover `application=keepassxc-control` keyring items are deleted
  best-effort. The `secret-service` crate exists only for that scrub.
- Session password is `Zeroizing` on the helper until process exit, path
  change, or a fingerprint open rejects it. Lock wipes the decrypted vault
  only.
- QML owns the clipboard clear timer. The helper must perform the write
  (Quickshell cannot close `wl-copy` stdin). Clear is
  `/usr/bin/wl-copy --clear`; compare is `/usr/bin/wl-paste`.
- Browse is `omarchy file select`. An in-process file dialog aborts the
  shell.
- Release builds always use `/usr/bin/fprintd-verify`.
  `KEEPASSXC_CONTROL_AUTH` is debug-only and ignored in release.
- A missing helper is a hard stop (“Run make”). A helper that dies is
  restarted with backoff and stops after five consecutive failures. Anyone
  who can write this plugin directory can replace the helper and receive
  typed passwords.

## Commands

```
make test      # helper tests + Model.test.js
make           # release helper → ./keepassxc-control-helper
make fixture   # regenerate fixtures/synthetic.kdbx
make reload    # rebuild helper and restart the shell
```

Fixture password is `spike-only-password`. Never reuse it.
