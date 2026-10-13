use crate::{Mode, Result};
use std::process::Command;

fn succeeds(command: &str, args: &[&str]) -> bool {
    Command::new(command)
        .args(args)
        .output()
        .is_ok_and(|output| output.status.success())
}

pub(crate) fn check(mode: Mode) -> Result<()> {
    println!(
        "LumenDesk doctor: {} / {} ({})",
        std::env::consts::OS,
        std::env::consts::ARCH,
        mode.as_str()
    );
    let mut missing = Vec::new();
    if succeeds("cargo", &["--version"]) {
        println!("OK cargo");
    } else {
        missing.push("cargo: install Rust 1.95 or newer from https://rustup.rs/ and add Cargo's bin directory to PATH".into());
    }
    match Command::new("rustc").arg("--version").output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout);
            let numbers: Vec<_> = version.split_whitespace().nth(1).unwrap_or("").split('.').take(2).collect();
            let major = numbers.first().and_then(|n| n.parse::<u32>().ok());
            let minor = numbers.get(1).and_then(|n| n.parse::<u32>().ok());
            if major.is_some_and(|major| major > 1) || (major == Some(1) && minor.is_some_and(|minor| minor >= 95)) {
                println!("OK {}", version.trim());
            } else { missing.push(format!("Rust 1.95 or newer is required; found {}. Run rustup update stable", version.trim())); }
        }
        _ => missing.push("rustc: install Rust 1.95 or newer from https://rustup.rs/ and add Cargo's bin directory to PATH".into()),
    }
    match std::env::consts::OS {
        "linux" => {
            if !succeeds("cc", &["--version"]) {
                missing.push("C compiler: install build-essential (Debian/Ubuntu) or your distribution's C development toolchain".into());
            }
            if !succeeds("pkg-config", &["--version"]) {
                missing.push("pkg-config: install your distribution's pkg-config package".into());
            }
            if mode == Mode::Web {
                for (library, package) in [
                    ("gtk+-3.0", "libgtk-3-dev"),
                    ("webkit2gtk-4.1", "libwebkit2gtk-4.1-dev"),
                ] {
                    if succeeds("pkg-config", &["--exists", library]) {
                        println!("OK {library}");
                    } else {
                        missing.push(format!("{library}: install {package} (Debian/Ubuntu) or equivalent development headers for your distribution"));
                    }
                }
            } else {
                for library in ["x11", "xkbcommon"] {
                    if succeeds("pkg-config", &["--exists", library]) {
                        println!("OK {library}");
                    } else {
                        println!(
                            "NOTE {library} development metadata not found; native builds/runtime may need libx11-dev and libxkbcommon-dev (Debian/Ubuntu)"
                        );
                    }
                }
            }
            if std::env::var_os("DISPLAY").is_none()
                && std::env::var_os("WAYLAND_DISPLAY").is_none()
            {
                println!(
                    "NOTE No DISPLAY or WAYLAND_DISPLAY; compilation may work, but opening a real window needs a graphical session."
                );
            }
        }
        "macos" => {
            if !succeeds("xcrun", &["--find", "clang"]) {
                missing
                    .push("Apple command-line developer tools: run xcode-select --install".into());
            }
            if mode == Mode::Web {
                println!(
                    "NOTE The system WKWebView is used; runtime behavior must be verified on this Mac."
                );
            }
        }
        "windows" => {
            println!(
                "NOTE Install Visual Studio Build Tools with Desktop development with C++ and the Windows SDK."
            );
            if mode == Mode::Web {
                println!(
                    "NOTE A WebView2 Runtime must be installed. Its presence is not verified by this tool: https://developer.microsoft.com/microsoft-edge/webview2/"
                );
            }
        }
        other => missing.push(format!(
            "unsupported desktop host '{other}'; this milestone targets Linux, macOS, and Windows"
        )),
    }
    if missing.is_empty() {
        println!(
            "Required command checks passed. This does not verify a graphical session, GPU, accessibility, or runtime behavior."
        );
        Ok(())
    } else {
        for issue in &missing {
            eprintln!("MISSING {issue}");
        }
        Err(format!(
            "{} prerequisite check(s) failed; address the MISSING items above",
            missing.len()
        ))
    }
}
