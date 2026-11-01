# Troubleshooting & Manual Runtime Guide

This guide explains how to troubleshoot common runtime issues in **OLS (Open Local Server)** and how to manually install or register any runtime (Apache, Nginx, PHP, Node.js, Python, MariaDB, PostgreSQL, etc.) if automatic downloads fail or if you are running in an offline or restricted environment.

---

## 1. Where OLS Stores Data & Runtimes

OLS stores its state, downloaded archives, and supervised runtimes in its `data\` directory:

* **Beside the Executable (Default for Portable & Installed Setups):**  
  Inside `<install dir>\data\`:
  ```text
  <install dir>\
  ├── OLS.exe
  ├── olsc.exe
  └── data\
       ├── cache\       <-- Downloaded runtime archives (.zip, .phar)
       ├── runtimes\     <-- Installed runtime binaries
       └── web\         <-- Generated web server configs & logs
  ```
* **Fallback AppData Location (if install directory is read-only):**  
  `%APPDATA%\OpenLocalServer\OpenLocalServer\`  
  *(Press <kbd>Win</kbd> + <kbd>R</kbd>, paste `%APPDATA%\OpenLocalServer\OpenLocalServer`, and press <kbd>Enter</kbd>)*

---

## 2. Why Add Runtimes Manually?

* **Checksum Mismatches & Upstream Rebuilds:** Vendors (such as Apache Lounge or PHP) occasionally recompile or update binaries on their servers (e.g. updating bundled OpenSSL or compiler toolchains) under the same release name. Because OLS strictly verifies SHA-256 signatures before extracting, any upstream hash change causes OLS to refuse the unverified binary.
* **Network Restrictions & Proxy Interference:** Corporate proxies, captive portals, or school firewalls intercepting downloads or returning HTML login/error pages instead of the archive, tripping the checksum check.
* **Vendor Download Issues:** Third-party hosts (like Apache Lounge) enforcing strict HTTP Referer headers, rate-limiting, or updating URLs.
* **Offline Environments:** Machines without direct internet access.
* **Antivirus False Positives:** Antivirus suites (like Windows Defender or enterprise scanners) heuristically quarantining standalone `.exe` downloads before OLS can extract them.
* **Custom Builds:** Using a custom-compiled binary or a specific patch version not in the default catalog.

---

## 3. How Manual Runtime Discovery Works

OpenLocalServer scans the `runtimes\<id>\` folder dynamically. Any folder named with a version string (e.g. `2.4.69`, `8.3.10`, `22.14.0`) that contains the expected executable is automatically recognized by OLS as an installed runtime.

### Binary Path Reference Table

To add a runtime manually, create the directory:
```text
<install dir>\data\runtimes\<id>\<version>\
```
Then extract the runtime files such that the main executable sits at the exact path listed below:

| Runtime / Service | ID (`<id>`) | Expected Binary Location |
| :--- | :--- | :--- |
| **Apache HTTP Server** | `apache` | `<version>\bin\httpd.exe` |
| **Nginx** | `nginx` | `<version>\nginx.exe` |
| **Caddy** | `caddy` | `<version>\caddy.exe` |
| **PHP** | `php` | `<version>\php.exe` *(and `php-cgi.exe`)* |
| **Node.js** | `node` | `<version>\node.exe` |
| **Python** | `python` | `<version>\python.exe` |
| **MariaDB** | `mariadb` | `<version>\bin\mariadbd.exe` |
| **PostgreSQL** | `postgres` | `<version>\bin\postgres.exe` |
| **MongoDB** | `mongodb` | `<version>\bin\mongod.exe` |
| **Redis** | `redis` | `<version>\redis-server.exe` |
| **Memcached** | `memcached` | `<version>\bin\memcached.exe` |
| **Composer** | `composer` | `<version>\composer.phar` |

---

## 4. Step-by-Step Example: Adding Apache Manually

If you run into an issue downloading Apache automatically:

1. **Download Apache for Windows:**
   * Visit [Apache Lounge Downloads](https://www.apachelounge.com/download/).
   * Download the latest **Win64** package (e.g. `httpd-2.4.69-261002-Win64-VS18.zip`).
2. **Create the Runtime Folder:**
   * Go to `<install dir>\data\runtimes\apache\`.
   * Create a subfolder with your version number (e.g. `2.4.69`).
3. **Extract the Files:**
   * Open the downloaded zip archive. Inside, you will see a folder named `Apache24`.
   * Extract the **contents** of `Apache24` directly into `<install dir>\data\runtimes\apache\2.4.69\`.
   * **Verify:** Ensure `httpd.exe` is located at:
     ```text
     <install dir>\data\runtimes\apache\2.4.69\bin\httpd.exe
     ```
     *(Avoid nested folders like `apache\2.4.69\Apache24\bin\httpd.exe`)*.
4. **Ensure Visual C++ Redistributable is Installed:**
   * Apache Lounge binaries require the **Microsoft Visual C++ 2015–2022 Redistributable (x64)**. If not already present on your Windows machine, download and run `vc_redist.x64.exe` from Microsoft.
5. **Switch to Apache in OLS:**
   * Restart OLS or navigate to the **Runtimes** page.
   * Go to **Web server** in the sidebar, choose **Apache**, and click **Apply Changes**.

---

## 5. Adding Existing Runtimes via the GUI (No File Moving)

If you already have runtimes installed on your machine (e.g. from Laragon, XAMPP, or manual installers), you can link them directly from the OLS user interface without copying files:

### Bulk Importing PHP Versions
1. Open OLS and click **Runtimes** in the sidebar.
2. In the **Add a runtime** card, click **Add folder**.
3. Choose the parent directory where your PHP versions live (e.g. `C:\laragon\bin\php`).
4. OLS automatically scans and registers every PHP version found with both `php.exe` and `php-cgi.exe`.

### Registering Single Executables (PHP, Node.js, Python)
1. Open OLS and click **Runtimes** in the sidebar.
2. Under **Add one executable**:
   * Select the runtime type (`php`, `node`, or `python`).
   * Type a version label (e.g. `8.3.10` or `20.15.0`).
   * Click **Locate** and select the `.exe` file on your drive.
3. The custom runtime is immediately available for site assignment and CLI commands.

---

## 6. The Cache Drop-In Method (Bypassing Download Failures)

If OLS fails mid-download due to a timeout or connection drop:

1. Download the required archive manually in your web browser.
2. Place the intact `.zip` or `.phar` directly into:
   ```text
   <install dir>\data\cache\
   ```
   *(Ensure the filename matches the package filename, e.g. `httpd-2.4.69-261002-Win64-VS18.zip`)*.
3. Return to OLS and click **Install** on the runtime in the **Runtimes** page.
4. OLS will detect the cached archive, verify its SHA-256 checksum, and extract it instantly without re-downloading.

---

## 7. Common Issues & Quick Fixes

### Problem: "checksum mismatch: expected ... got ... — refusing to install an unverified binary"
* **Cause:**
  * **Upstream Recompile / Update:** The vendor (e.g. Apache Lounge) rebuilt or updated their zip distribution on their server without changing the version number, causing the actual SHA-256 to change.
  * **Interrupted / Truncated Download:** A network drop cut the transfer short, resulting in a partial archive with an unexpected hash.
  * **Proxy / Captive Portal:** A corporate firewall, proxy, or Wi-Fi login screen intercepted the download request and returned an HTML error/login page instead of the zip file.
* **Fix:**
  * **Use the manual drop-in method:** Download the binary manually from the official source in your browser, and extract it directly into `<install dir>\data\runtimes\<id>\<version>\` as detailed in [Section 4](#4-step-by-step-example-adding-apache-manually). OLS directly detects the executable and bypasses the download checksum gate.
  * Alternatively, check if a captive portal or proxy is intercepting web requests.

### Problem: Apache fails to start (`VCRUNTIME140.dll` / `MSVCP140.dll was not found`)
* **Cause:** The required Microsoft Visual C++ runtime library is missing from Windows.
* **Fix:** Download and install the latest **Visual C++ 2015–2022 Redistributable (x64)** from Microsoft's official support site.

### Problem: Web server port clash (Port 80 or 443 already bound)
* **Cause:** Another process (such as IIS, World Wide Web Publishing Service, or Skype) is holding port 80 or 443.
* **Fix:**
  * In OLS, go to **Web server** > **Ports** and set Apache/Nginx to custom ports (e.g. 8080 / 8443).
  * Or stop the conflicting service via PowerShell:
    ```powershell
    Stop-Service W3SVC -Force
    ```

### Problem: "No such file or directory" during extraction or disappearing `.exe`
* **Cause:** Windows Defender or third-party antivirus quarantined the standalone server executable via heuristic detection.
* **Fix:**
  * Check your antivirus protection history and restore the file.
  * Add an exclusion for the `<install dir>\data\runtimes` folder in Windows Defender.

### Problem: Missing project folder badge ("Folder missing" or "Drive offline")
* **Cause:** A project folder was renamed, moved, or sits on a disconnected USB/network drive.
* **Fix:**
  * Open the **Sites** page.
  * Click **Locate** on the flagged site row to point OLS to the new location.
  * The web server and site configurations will automatically regenerate.
