# Cutover from the Python app (M8)

The one switch from the Python app and stock ShardBrowser to our app, and how to undo it. It runs once, at M8 (D-M4-1), on the owner's Windows machine, with the owner present. The data comes over by hand (D-M5-4): with both apps closed, `%LOCALAPPDATA%\ghidorah-creator\ghidorah.sqlite` is copied to `%LOCALAPPDATA%\shardx-launcher\creator.sqlite`. Our store is Python's schema v24 verbatim, so the copy opens as our store. Browser profiles stay where they are: our app takes over stock Shard's data folder (same app id), so the profile ids in the Python rows resolve as before.

## 1. Preconditions

All must hold; otherwise stop here.

- Every provider is ported and live-gated in our app (Qwen, Noiz, Fish Audio, Grok, ChatGPT, ElevenLabs, Google and the "via Google" routes).
- Our installer is built (M8), and the store checks (`live_perf`, `live_profiles_check`, `live_open_imported` in `windows-build.md`) were repeated with this build on a fresh copy of the Python database: every `live_perf` query OK.
- The Python database is at `user_version` 24 (our app refuses any other version).
- The installed stock ShardBrowser version is noted and its installer kept. Our build's version is not lower (a lower one would downgrade Shard's data).
- Free disk: at least 2x the size of `ghidorah.sqlite` on the `%LOCALAPPDATA%` drive (the copy), plus room for the backups in step 4.
- The owner is present.

## 2. Quiesce the Python app

1. Stop every job; wait until no account is making.
2. Close the Python app normally (never end its process).
3. Check that no `ghidorah.sqlite-wal` is left beside `ghidorah.sqlite`, or that it is 0 bytes: a clean close leaves none. If one stays, start the Python app once and close it again normally. Only if that fails, copy the `-wal` along in step 6 (never the `-shm`).

## 3. Close stock ShardBrowser

Quit it normally (tray or menu Quit) with no profile running. Check no ShardBrowser or ShardX browser process is left.

## 4. Back up

Copy each folder to a backup location, then compare the file count and total size of copy and original before going on:

- `%LOCALAPPDATA%\ghidorah-creator` (the Python database, with its `-wal` if present);
- `%APPDATA%\shardx-launcher` (settings, `proxies.json`, profiles, user data, extensions);
- `%LOCALAPPDATA%\shardx-launcher`;
- Shard's data location if it was moved in Shard's Settings (its `profiles`, `user-data`, `extensions` folders).

## 5. Install ours

Install over stock ShardBrowser (same app id: an in-place upgrade). If stock must be uninstalled first, uninstall it **without** "delete app data". Never tick "delete app data".

## 6. Copy the store

Before the first start, with our app not running:

1. If `%LOCALAPPDATA%\shardx-launcher\creator.sqlite` exists (a dev or rehearsal run), move it aside, with its `-wal` and `-shm`.
2. Copy `%LOCALAPPDATA%\ghidorah-creator\ghidorah.sqlite` to `%LOCALAPPDATA%\shardx-launcher\creator.sqlite` (and its `-wal` to `creator.sqlite-wal` only in the case of step 2.3).

## 7. First start

1. Start our app. The first open adds the sender index (seconds on a large mail table), and the startup sweep fails as Interrupted every row Python left queued or running (0 expected after step 2).
2. Check the Accounts (per platform), Emails (type and status) and proxy counts against the Python app's last screens.
3. Shard's All profiles count equals the count before the cutover; the folder "Ghidorah" is there.
4. A store that does not open (the error shows on our pages) means a wrong file or version: roll back (step 10).

## 8. Verify live

1. Test 20 Outlook mailboxes on the Emails page.
2. Open 2 copied accounts' profiles from Shard: signed in, timezone of their proxy.
3. Make 1 Breeze Blue account on a free Outlook mailbox.
4. Make 1 account with each other provider.

A failure here: roll back (step 10) or fix forward, the owner's call.

## 9. Retire the Python app

Uninstall it and remove its shortcut. Keep the step 4 backup of its data folder until the owner deletes it.

## 10. Rollback

Possible at any time before the owner declares the cutover done.

1. Close our app normally.
2. If anything done in our app since the cutover should be kept, move `%LOCALAPPDATA%\shardx-launcher\creator.sqlite` (with its `-wal`) aside first; otherwise delete our `creator.sqlite` (with its `-wal` and `-shm`).
3. Uninstall our app, without "delete app data".
4. Restore the step 4 folders over the current ones: replace, not merge (delete the current folder, then copy the backup in).
5. Reinstall stock ShardBrowser from the kept installer.
6. Start the Python app. Its database was only copied, never written, so it continues where it stopped.
