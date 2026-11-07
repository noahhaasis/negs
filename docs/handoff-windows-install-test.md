# Handoff (Windows): test the installer and uninstaller for `win-nov5`

**Who:** a Windows agent.
**Why:** the Nov 5 port (`win-nov5`) packaged only the portable zip. The installer,
`test_uninstall.ps1` and the full `verify_package.py` run were never done, because this machine
has no Inno Setup and no Python 3.11. Close that gap.

## Inputs

- **Branch:** `win-nov5`, head `f04fb4b` or later.
  - Worktree: `C:\Users\Noah\Projects\negs-win-nov5`.
  - Not pushed or merged. Package from that committed tree only, so source and binary match.
  - Don't create commits there unless a fix is needed.
- **Docs:** "Packaging" and the uninstaller paragraphs at the end of `docs/windows-app.md`. Also
  the checklist in `docs/windows-ui-validation.md`.
- **Scripts:**
  - `windows/package.ps1`, with `-Iscc` for the installer.
  - `windows/installer.iss`.
  - `windows/verify_package.py`.
  - `windows/test_uninstall.ps1`.
- **Last full pass:** 2026-10-24, with Inno Setup 6.7.3 and all 35 association install/uninstall
  checks.

## Setup (ask Noah before installing anything)

1. **Inno Setup 6**, from the official jrsoftware.org installer. 6.7.3 matches the last pass.
2. **Python 3.11 or newer**, for `verify_package.py`.
3. **A disposable Windows user profile.** `test_uninstall.ps1` refuses to run if any of these
   exist: `%APPDATA%\Negs`, `%LOCALAPPDATA%\Negs`, `%TEMP%\Negs`, `HKCU\Software\Negs` or an
   existing install.
   - Never run it in Noah's own profile.
   - Never delete his app data to make it pass.
   - If no disposable profile exists, stop and ask Noah to create a local test user and sign
     into it, or run you there.

## Steps

1. **Build.** In the worktree:
   `powershell -File windows/package.ps1 -Iscc "<path>\ISCC.exe"`
   - Expect `target/windows-dist/Negs-portable.zip`, about 165 MB, and `Negs-Setup.exe`.
   - Record both sizes and their SHA-256.
2. **Verify.** Run `python windows/verify_package.py` with 3.11, using its documented
   arguments.
   - It checks archive integrity, the bundled executables against the committed source, and
     version agreement between the workspace, the app and the installer.
   - Every check must pass. Report any version mismatch rather than editing versions.
3. **Install/uninstall test.** In the disposable profile, 64-bit PowerShell:
   `powershell -File windows/test_uninstall.ps1 -SetupExe target/windows-dist/Negs-Setup.exe`
   - It covers upgrade preservation, running-app protection, both cleanup modes
     (`/KEEPSETTINGS=0` and `1`), and that fixture photos, exports and unrelated files survive.
   - Report pass/fail per check, with output.
4. **Manual smoke test of the installed app**, in the disposable profile:
   - **Start Menu:** launch it from there.
   - **Photo and films:** open a RAW, pick films from the grid and through "Search films".
   - **Matching:** with matching on, check that a film click shows the previous frame, then the
     matched quick frame, then the settled frame.
   - **Saving:** Save Preview, then Fast export.
   - **Open With:** right-click a RAW and use Open With → Negs. The app must not become the
     default.
   - **Uninstall:** quit, then uninstall via Settings → Apps. Use both "Keep settings" and
     "Remove all app data" (reinstall in between), and confirm what remains matches the docs.
5. **Report.** Add a dated "Installer validation" note to `docs/windows-app.md` (or
   `windows-ui-validation.md`):
   - date, machine, commit
   - Inno and Python versions
   - sizes and hashes
   - results per check
   - anything not run

   Commit the note on `win-nov5` with the trailer
   `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Rules

- **No push and no merge.** Don't push or merge anything unless Noah says so in chat.
- **Report failures honestly.**
  - A failing check is a finding: say where and why. Fix it only if the cause is clearly in the
    packaging scripts.
  - Keep any fix to its own commit, and say so.
- **Nothing in Noah's own folders.** Don't write into his Downloads, Documents or Pictures. Point
  exports at a temp folder.
