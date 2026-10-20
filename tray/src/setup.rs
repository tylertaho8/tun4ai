// First-run setup for the downloaded app. The tray downloads sing-box itself;
// only the last step asks for an admin password, and it runs the copy of
// install-root.sh that shipped inside the app bundle.

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub struct Bundle {
    pub fetch: PathBuf,
    pub install_root: PathBuf,
    pub examples: PathBuf,
}

// MacOS/tun4ai -> Contents/Resources, for an executable inside a bundle.
pub fn resources_of(exe: &Path) -> Option<PathBuf> {
    let resources = exe.parent()?.parent()?.join("Resources");
    resources.is_dir().then_some(resources)
}

pub fn bundle(resources: &Path) -> Option<Bundle> {
    let bundled = Bundle {
        fetch: resources.join("fetch-sing-box.sh"),
        install_root: resources.join("helper/install-root.sh"),
        examples: resources.join("examples"),
    };
    let ready = bundled.fetch.is_file()
        && bundled.install_root.is_file()
        && bundled.examples.join("vless.txt").is_file()
        && bundled.examples.join("direct.txt").is_file();
    ready.then_some(bundled)
}

pub fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

// One AppleScript statement. `prompt` is shown on the password dialog.
pub fn admin_command(install_root: &Path, owner: &str, config_dir: &Path, sing_box: &Path, lang: &str, prompt: &str) -> String {
    let lang = if lang == "zh" { "zh" } else { "en" };
    let inner = format!(
        "/bin/bash {} {} {} {} {}",
        shell_quote(&install_root.display().to_string()),
        shell_quote(owner),
        shell_quote(&config_dir.display().to_string()),
        shell_quote(&sing_box.display().to_string()),
        lang,
    );
    format!(
        "do shell script \"{}\" with administrator privileges with prompt \"{}\"",
        apple_escape(&inner),
        apple_escape(prompt),
    )
}

fn apple_escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

// Creates the config dir and copies the examples once. Existing files stay as the user left them.
pub fn prepare_config(examples: &Path, config_dir: &Path) -> io::Result<()> {
    fs::create_dir_all(config_dir)?;
    let mut dir = fs::metadata(config_dir)?.permissions();
    dir.set_mode(0o700);
    fs::set_permissions(config_dir, dir)?;
    for (name, mode) in [("vless.txt", 0o600), ("direct.txt", 0o644)] {
        let dest = config_dir.join(name);
        if dest.exists() {
            continue;
        }
        fs::copy(examples.join(name), &dest)?;
        let mut perm = fs::metadata(&dest)?.permissions();
        perm.set_mode(mode);
        fs::set_permissions(&dest, perm)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_command_quotes_paths_and_locks_the_language() {
        let script = admin_command(
            Path::new("/Apps/tun4ai.app/Contents/Resources/helper/install-root.sh"),
            "ada",
            Path::new("/tmp/config dir"),
            Path::new("/tmp/tun4ai setup/sing-box"),
            "zh-Hant",
            "Install tun4ai",
        );
        assert!(script.starts_with("do shell script \"/bin/bash "));
        assert!(script.contains("'/tmp/config dir'"));
        assert!(script.contains("'/tmp/tun4ai setup/sing-box'"));
        assert!(script.contains(" en\""));
        assert!(script.ends_with("with prompt \"Install tun4ai\""));
        assert!(!script.contains("zh-Hant"));
    }

    #[test]
    fn shell_quote_survives_a_single_quote() {
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn prepare_config_copies_once_and_keeps_edits() {
        let root = std::env::temp_dir().join(format!("tun4ai-setup-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let examples = root.join("examples");
        fs::create_dir_all(&examples).unwrap();
        fs::write(examples.join("vless.txt"), "example-node\n").unwrap();
        fs::write(examples.join("direct.txt"), "10.0.0.0/8\n").unwrap();
        let config = root.join("config dir");
        prepare_config(&examples, &config).unwrap();
        assert_eq!(fs::metadata(&config).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(fs::metadata(config.join("vless.txt")).unwrap().permissions().mode() & 0o777, 0o600);
        fs::write(config.join("vless.txt"), "mine\n").unwrap();
        fs::write(examples.join("vless.txt"), "changed\n").unwrap();
        prepare_config(&examples, &config).unwrap();
        assert_eq!(fs::read_to_string(config.join("vless.txt")).unwrap(), "mine\n");
        assert_eq!(fs::read_to_string(config.join("direct.txt")).unwrap(), "10.0.0.0/8\n");
        let _ = fs::remove_dir_all(&root);
    }
}
