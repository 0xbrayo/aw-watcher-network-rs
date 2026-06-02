use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() != "macos" {
        return;
    }

    println!("cargo:rerun-if-changed=src/wifi_helper/main.swift");
    println!("cargo:rerun-if-changed=src/wifi_helper/Info.plist");

    if env::var("SKIP_WIFI_HELPER").is_ok() {
        println!("cargo:warning=SKIP_WIFI_HELPER set — skipping aw-wifi-helper build");
        return;
    }

    // Require swiftc (part of Xcode Command Line Tools)
    let has_swiftc = Command::new("which")
        .arg("swiftc")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !has_swiftc {
        println!(
            "cargo:warning=swiftc not found — Wi-Fi SSID scanning will be unavailable on macOS (run: xcode-select --install)"
        );
        return;
    }

    // OUT_DIR = .../target/<profile>/build/<crate>-<hash>/out
    // We want .../target/<profile>/ to place the .app next to the main binary.
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let profile_dir = out_dir
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .expect("unexpected OUT_DIR structure");

    let helper_binary = out_dir.join("aw-wifi-helper");

    let status = Command::new("swiftc")
        .args([
            "src/wifi_helper/main.swift",
            "-framework",
            "CoreWLAN",
            "-framework",
            "CoreLocation",
            "-framework",
            "Foundation",
            "-o",
        ])
        .arg(&helper_binary)
        .status();

    match status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            println!(
                "cargo:warning=Swift helper compilation failed (exit {s}) — Wi-Fi SSID scanning will be limited on macOS"
            );
            return;
        }
        Err(e) => {
            println!(
                "cargo:warning=Failed to invoke swiftc: {e} — Wi-Fi SSID scanning will be limited on macOS"
            );
            return;
        }
    }

    // Assemble the .app bundle next to the main binary
    let app_bundle = profile_dir.join("aw-wifi-helper.app");
    let app_macos = app_bundle.join("Contents").join("MacOS");

    fs::create_dir_all(&app_macos).expect("failed to create app bundle dirs");

    fs::copy(&helper_binary, app_macos.join("aw-wifi-helper"))
        .expect("failed to copy helper binary into .app");

    fs::copy(
        "src/wifi_helper/Info.plist",
        app_bundle.join("Contents").join("Info.plist"),
    )
    .expect("failed to copy Info.plist into .app");

    // Sign with the best available identity so TCC can persist location auth.
    // Preference: exact "aw-wifi-helper" cert hash → any Developer ID → ad-hoc.
    // Ad-hoc ("-") means TCC revokes the grant on every rebuild.
    let sign_identity = if let Some(id) = find_codesign_identity("aw-wifi-helper") {
        id
    } else if let Some(dev_id) = find_developer_id() {
        dev_id
    } else {
        println!(
            "cargo:warning=No stable code-signing identity found — Location Services auth will be revoked on each rebuild. Create a 'Code Signing' certificate named 'aw-wifi-helper' in Keychain Access."
        );
        "-".to_string()
    };

    let _ = Command::new("codesign")
        .args(["--force", "--sign"])
        .arg(&sign_identity)
        .arg(&app_bundle)
        .status();

    // Also install to a stable user-level path so the main binary works regardless
    // of where it's been copied/installed.
    if let Some(install_dir) = stable_install_dir() {
        let install_app = install_dir.join("aw-wifi-helper.app");
        let install_macos = install_app.join("Contents").join("MacOS");
        if fs::create_dir_all(&install_macos).is_ok()
            && fs::copy(
                app_macos.join("aw-wifi-helper"),
                install_macos.join("aw-wifi-helper"),
            )
            .is_ok()
            && fs::copy(
                app_bundle.join("Contents").join("Info.plist"),
                install_app.join("Contents").join("Info.plist"),
            )
            .is_ok()
        {
            let _ = Command::new("codesign")
                .args(["--force", "--sign"])
                .arg(&sign_identity)
                .arg(&install_app)
                .status();
            println!(
                "cargo:warning=Installed aw-wifi-helper.app → {}",
                install_app.display()
            );
        }
    }

    println!(
        "cargo:warning=Built aw-wifi-helper.app → {}",
        app_bundle.display()
    );
}

/// Returns ~/Library/Application Support/aw-watcher-network, creating it if needed.
fn stable_install_dir() -> Option<PathBuf> {
    let home = env::var("HOME").ok()?;
    let dir = PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("aw-watcher-network");
    fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Returns the SHA-1 hash of the first code-signing identity matching `name`,
/// or None if no such identity exists. Using the hash avoids ambiguity when
/// multiple certs share a name.
fn find_codesign_identity(name: &str) -> Option<String> {
    let out = Command::new("security")
        .args(["find-identity", "-v", "-p", "codesigning"])
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let needle = format!("\"{}\"", name);
    for line in stdout.lines() {
        if line.contains(&needle) {
            // Line format:  "  1) <HASH> "name""
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let hash = parts[1].trim_matches('"');
                // Validate it looks like a hex SHA-1
                if hash.len() == 40 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Some(hash.to_string());
                }
            }
        }
    }
    None
}

fn find_developer_id() -> Option<String> {
    let out = Command::new("security")
        .args(["find-identity", "-v", "-p", "codesigning"])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find(|l| l.contains("Developer ID Application"))
        .and_then(|l| {
            let start = l.find('"')? + 1;
            let end = l.rfind('"')?;
            Some(l[start..end].to_string())
        })
}
