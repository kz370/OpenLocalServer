# OpenLocalServer Update Flow Testing Toolkit

This folder contains a complete local test environment for testing OpenLocalServer's end-to-end update system:
**Check for updates -> Download installer -> Verify minisign signature & SHA-256 -> Install update**.

---

## Files in this Directory

| File | Purpose |
|------|---------|
| [`serve.py`](file:///i:/Development/repos/OpenLocalServer/testings/serve.py) | Python test server: hosts files, opens HTTPS tunnel, signs manifest, configures `settings.json` |
| [`run-test-server.ps1`](file:///i:/Development/repos/OpenLocalServer/testings/run-test-server.ps1) | PowerShell one-click launcher for `serve.py` |
| [`fake-installer.exe`](file:///i:/Development/repos/OpenLocalServer/testings/fake-installer.exe) | Real compiled Windows test executable that shows a success dialog upon installation |
| [`installer_src.rs`](file:///i:/Development/repos/OpenLocalServer/testings/installer_src.rs) | Rust source for `fake-installer.exe` |
| [`update-test.pub`](file:///i:/Development/repos/OpenLocalServer/testings/update-test.pub) | Test Minisign public key |
| [`update-test.key`](file:///i:/Development/repos/OpenLocalServer/testings/update-test.key) | Test Minisign secret signing key |

---

## How to Test

### Step 1: Start the Update Test Server
Open a terminal and run:
```powershell
.\testings\run-test-server.ps1
```
*(or `python testings/serve.py`)*

The script will:
1. Start a local HTTP server on port `9876`.
2. Launch a free Cloudflare tunnel (`cloudflared`) to provide a valid, trusted `https://*.trycloudflare.com` URL (required by `ols-core`'s strict HTTPS and Mozilla root CA verification).
3. Generate and sign `latest.json` pointing to `fake-installer.exe`.
4. Automatically update `data/settings.json` so OLS points directly to this test endpoint.

### Step 2: Test in the OLS Application
In the running OpenLocalServer window:
1. Go to **Settings** -> **General**.
2. Scroll to the **Updates** card:
   - Click **Check for updates**.
   - The card will immediately show: **`v99.9.9 is available`** with the test release notes.
3. Click **Download update**:
   - The progress spinner runs while it downloads `fake-installer.exe`.
   - The SHA-256 checksum is checked and verified against the manifest.
   - The card changes to **`Installer downloaded and verified. Ready to install v99.9.9.`**
4. Click **Install now**:
   - The confirmation prompt appears.
   - Click **Install**.
   - The application launches `fake-installer.exe` and closes, and the test installer popup appears!

---

## Administrator Helper Note

If the Administrator helper displays:
`Installed · version 2.1.0 (app is 2.2.0)`

The helper executable (`target/debug/ols-helper.exe`) has now been compiled for version 2.2.0. Click **Update (asks once)** and confirm the Windows UAC prompt to update the service.
