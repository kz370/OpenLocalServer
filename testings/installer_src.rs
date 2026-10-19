use std::process::Command;

fn main() {
    // Show a message box using powershell or cmd so we don't need external crates
    let _ = Command::new("powershell")
        .args(["-NoProfile", "-Command", "[System.Windows.Forms.MessageBox]::Show('OLS Test Installer executed successfully! Version 99.9.9.', 'OpenLocalServer Update Test', 'OK', 'Information')"])
        .status();
}
