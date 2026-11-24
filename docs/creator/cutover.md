# Cutover from the Python app (M8)

The one switch from the Python app and stock ShardBrowser to our app, and how to undo it. It runs once, at M8 (D-M4-1), on the owner's Windows machine, with the owner present. Our app imports the Python database by itself on its first start (`store::open_or_import`): `%LOCALAPPDATA%\ghidorah-creator\ghidorah.sqlite` is copied, never opened, and becomes `%LOCALAPPDATA%\shardx-launcher\creator.sqlite`. Browser profiles stay where they are: our app takes over stock Shard's data folder (same app id), so the profile ids in the Python rows resolve as before.

## 1. Preconditions

All must hold; otherwise stop here.

- Every provider is ported and live-gated in our app (Qwen, Noiz, Fish Audio, Grok, ChatGPT, ElevenLabs, Google and the "via Google" routes).
- Our installer is built (M8), and the M4 rehearsal (`docs/plans/2026-11-24-creator-m4-data.md`, Task 3) was repeated with this build on a fresh backup of the Python database: same counts as the Python app, every `live_perf` query OK.
- The Python database is at `user_version` 24 (the rehearsal's backup shows it).
- The installed stock ShardBrowser version is noted and its installer kept. Our build's version is not lower (a lower one would downgrade Shard's data).
- Free disk: at least 3x the size of `ghidorah.sqlite` on the `%LOCALAPPDATA%` drive (import temp copy + result), plus room for the backups in step 4.
- The owner is present.

## 2. Quiesce the Python app

1. Stop every job; wait until no account is making.
2. Close the Python app normally (never end its process).
3. Accounts must show 0 making: our import cannot finish rows Python left mid-run (no startup sweep before M5). If any show, start the Python app once, let its own startup sweep run, and close it again.

## 3. Close stock ShardBrowser

Quit it normally (tray or menu Quit) with no profile running. Check no ShardBrowser or ShardX browser process is left.

## 4. Back up

Copy each folder to a backup location, then compare the file count and total size of copy and original before going on:

- `%LOCALAPPDATA%\ghidorah-creator` (the Python database, with its `-wal` if present);
- `%APPDATA%\shardx-launcher` (settings, `proxies.json`, profiles, user data, extensions);
- `%LOCALAPPDATA%\shardx-launcher`;
- Shard's data location if it was moved in Shard's Settings (its `profiles`, `user-data`, `extensions` folders).

## 5. Install ours

1. Check that `%LOCALAPPDATA%\shardx-launcher\creator.sqlite` does not exist. If it does (a dev or rehearsal run), move it aside: an existing store is never replaced, so the import would not run.
2. Install over stock ShardBrowser (same app id: an in-place upgrade). If stock must be uninstalled first, uninstall it **without** "delete app data". Never tick "delete app data".

## 6. First start

1. Start our app. The import runs in the background: about one to two minutes for a 1.2 GB database; our pages say "Importing the Python database…" until it ends, then need a refresh.
2. Check `%LOCALAPPDATA%\shardx-launcher\creator-import.json` exists and was written today; if not, the import did not run (see step 5.1). Its counts must equal the Python app's last screens (Emails type and status, Accounts per platform, Proxies), and `live_accounts` must be 0.
3. An import error shows on our pages and leaves no `creator.sqlite`; the next start retries. Fix the cause (disk space, a database not at version 24) or roll back.
4. Shard's All profiles count equals the count before the cutover; the folder "Ghidorah" is there.

## 7. Verify live

1. Test 20 Outlook mailboxes on the Emails page.
2. Open 2 imported accounts' profiles from Shard: signed in, timezone of their proxy.
3. Make 1 Breeze Blue account on an imported free Outlook mailbox.
4. Make 1 account with each other provider.

A failure here: roll back (step 9) or fix forward, the owner's call.

## 8. Retire the Python app

Uninstall it and remove its shortcut. Keep the step 4 backup of its data folder until the owner deletes it.

## 9. Rollback

Possible at any time before the owner declares the cutover done.

1. Close our app normally.
2. If anything done in our app since the cutover should be kept, move `%LOCALAPPDATA%\shardx-launcher\creator.sqlite` (with its `-wal`) aside first; otherwise it is lost.
3. Uninstall our app, without "delete app data".
4. Restore the step 4 folders over the current ones: replace, not merge (delete the current folder, then copy the backup in).
5. Reinstall stock ShardBrowser from the kept installer.
6. Start the Python app. Its database was only read, so it continues where it stopped.
