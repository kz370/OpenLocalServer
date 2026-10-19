"""
OLS Updater Local Test Server
=============================
This script:
1. Starts a local HTTP server serving the files in testings/
2. Opens a free Cloudflare HTTPS tunnel to satisfy reqwest HTTPS + trusted root CA requirements
3. Generates testings/latest.json with the HTTPS installer URL and genuine SHA256 checksum
4. Signs latest.json with testings/update-test.key using minisign
5. Updates data/settings.json so OLS points directly at this test endpoint
"""

import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import threading
import time
from http.server import SimpleHTTPRequestHandler, HTTPServer
from pathlib import Path

ROOT_DIR = Path(__file__).resolve().parent.parent
TESTINGS_DIR = ROOT_DIR / "testings"
DATA_SETTINGS = ROOT_DIR / "data" / "settings.json"
PORT = 9876

class QuietHandler(SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(TESTINGS_DIR), **kwargs)

    def log_message(self, format, *args):
        # Log download requests cleanly
        sys.stdout.write(f"[HTTP] {self.address_string()} - {format % args}\n")
        sys.stdout.flush()

def compute_sha256_and_size(path: Path):
    h = hashlib.sha256()
    size = 0
    with open(path, "rb") as f:
        while chunk := f.read(65536):
            h.update(chunk)
            size += len(chunk)
    return h.hexdigest().lower(), size

def find_cloudflared():
    for p in [
        r"C:\Program Files (x86)\cloudflared\cloudflared.exe",
        r"C:\Program Files\cloudflared\cloudflared.exe",
        shutil.which("cloudflared")
    ]:
        if p and os.path.exists(p):
            return p
    return None

def main():
    print("==================================================")
    print("       OpenLocalServer Update Test Server         ")
    print("==================================================")

    fake_installer = TESTINGS_DIR / "fake-installer.exe"
    if not fake_installer.exists():
        print(f"Error: {fake_installer} not found! Compile it first.")
        sys.exit(1)

    sha256, size = compute_sha256_and_size(fake_installer)
    print(f"Installer: fake-installer.exe ({size} bytes, sha256: {sha256[:16]}...)")

    pub_key_file = TESTINGS_DIR / "update-test.pub"
    sec_key_file = TESTINGS_DIR / "update-test.key"
    if not pub_key_file.exists() or not sec_key_file.exists():
        print("Error: Minisign keys not found in testings/!")
        sys.exit(1)

    pub_key_lines = pub_key_file.read_text(encoding="utf-8").strip().splitlines()
    pub_key = pub_key_lines[-1].strip()
    print(f"Public Key: {pub_key}")

    # Start HTTP server
    server = HTTPServer(("127.0.0.1", PORT), QuietHandler)
    server_thread = threading.Thread(target=server.serve_forever, daemon=True)
    server_thread.start()
    print(f"Local HTTP server listening on http://127.0.0.1:{PORT}")

    # Start cloudflared tunnel
    cf_bin = find_cloudflared()
    if not cf_bin:
        print("Error: cloudflared was not found in PATH or standard Program Files locations.")
        sys.exit(1)

    print("Starting Cloudflare tunnel (quick tunnel for HTTPS)...")
    cf_proc = subprocess.Popen(
        [cf_bin, "tunnel", "--url", f"http://127.0.0.1:{PORT}"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1
    )

    tunnel_url = None
    url_pattern = re.compile(r"https://[a-zA-Z0-9-]+\.trycloudflare\.com")

    # Read stderr of cloudflared to find the assigned tunnel URL
    def monitor_stderr():
        nonlocal tunnel_url
        for line in cf_proc.stderr:
            m = url_pattern.search(line)
            if m and not tunnel_url:
                tunnel_url = m.group(0)

    t = threading.Thread(target=monitor_stderr, daemon=True)
    t.start()

    print("Waiting for Cloudflare tunnel URL...")
    for _ in range(60):
        if tunnel_url:
            break
        time.sleep(0.5)

    if not tunnel_url:
        print("Error: Could not retrieve tunnel URL from cloudflared.")
        cf_proc.terminate()
        sys.exit(1)

    print(f"Tunnel URL established: {tunnel_url}")

    # Create latest.json
    manifest = {
        "version": "99.9.9",
        "notes": "Test update — fake release for update flow testing.",
        "pub_date": time.strftime("%Y-%m-%d"),
        "platforms": {
            "windows-x86_64": {
                "url": f"{tunnel_url}/fake-installer.exe",
                "sha256": sha256,
                "size": size
            },
            "windows-aarch64": {
                "url": f"{tunnel_url}/fake-installer.exe",
                "sha256": sha256,
                "size": size
            }
        }
    }

    latest_json_path = TESTINGS_DIR / "latest.json"
    latest_json_path.write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    print(f"Generated {latest_json_path}")

    # Sign latest.json with minisign
    sig_path = TESTINGS_DIR / "latest.json.minisig"
    if sig_path.exists():
        sig_path.unlink()

    minisign_bin = shutil.which("minisign") or "minisign"
    sign_cmd = [minisign_bin, "-S", "-s", str(sec_key_file), "-m", str(latest_json_path), "-W"]
    res = subprocess.run(sign_cmd, cwd=str(TESTINGS_DIR), capture_output=True, text=True)
    if res.returncode != 0:
        print(f"Error signing manifest: {res.stderr}")
        cf_proc.terminate()
        sys.exit(1)
    print("Signed latest.json -> latest.json.minisig")

    # Update data/settings.json
    endpoint_url = f"{tunnel_url}/latest.json"
    backup_updater_settings = None

    if DATA_SETTINGS.exists():
        try:
            settings_data = json.loads(DATA_SETTINGS.read_text(encoding="utf-8"))
            backup_updater_settings = settings_data.get("updater")
            settings_data["updater"] = {
                "settings": {
                    "endpoint": endpoint_url,
                    "public_key": pub_key
                },
                "last": None
            }
            DATA_SETTINGS.write_text(json.dumps(settings_data, indent=2), encoding="utf-8")
            print(f"Updated {DATA_SETTINGS} with test endpoint and public key.")
        except Exception as e:
            print(f"Warning: Could not update {DATA_SETTINGS}: {e}")

    # Update data/app.db
    app_db = ROOT_DIR / "data" / "app.db"
    backup_db_updater = None
    if app_db.exists():
        try:
            import sqlite3
            conn = sqlite3.connect(app_db)
            row = conn.execute("SELECT value FROM settings WHERE key='updater'").fetchone()
            if row:
                backup_db_updater = row[0]
            val = json.dumps({
                "settings": {
                    "endpoint": endpoint_url,
                    "public_key": pub_key
                },
                "last": None
            })
            conn.execute("INSERT INTO settings(key, value) VALUES('updater', ?) ON CONFLICT(key) DO UPDATE SET value=excluded.value", (val,))
            conn.commit()
            conn.close()
            print(f"Updated {app_db} with test endpoint and public key.")
        except Exception as e:
            print(f"Warning: Could not update {app_db}: {e}")

    print("\n" + "=" * 50)
    print("READY TO TEST IN OPENLOCALSERVER!")
    print("=" * 50)
    print(f"Endpoint:   {endpoint_url}")
    print(f"Public Key: {pub_key}")
    print("\nTesting Steps:")
    print("1. In OLS Settings -> General -> Updates:")
    print("   Click 'Configure source' and verify the test endpoint & public key.")
    print("   Click 'Save settings' (this applies them to the running app).")
    print("2. Click 'Check for updates'")
    print("   -> It will discover v99.9.9 available!")
    print("3. Click 'Download update'")
    print("   -> It will download fake-installer.exe and verify the SHA256")
    print("4. Click 'Install now'")
    print("   -> Confirm the modal, and the test installer popup will appear!")
    print("=" * 50)
    print("\nPress Ctrl+C to stop the test server and tunnel...")

    try:
        while True:
            time.sleep(1)
    except KeyboardInterrupt:
        print("\nStopping test server and tunnel...")
    finally:
        cf_proc.terminate()
        server.shutdown()
        # Restore settings if backed up
        if DATA_SETTINGS.exists() and backup_updater_settings is not None:
            try:
                settings_data = json.loads(DATA_SETTINGS.read_text(encoding="utf-8"))
                settings_data["updater"] = backup_updater_settings
                DATA_SETTINGS.write_text(json.dumps(settings_data, indent=2), encoding="utf-8")
                print("Restored original updater settings in settings.json.")
            except Exception:
                pass
        if app_db.exists() and backup_db_updater is not None:
            try:
                import sqlite3
                conn = sqlite3.connect(app_db)
                conn.execute("UPDATE settings SET value=? WHERE key='updater'", (backup_db_updater,))
                conn.commit()
                conn.close()
                print("Restored original updater settings in app.db.")
            except Exception:
                pass
        print("Done.")

if __name__ == "__main__":
    main()
