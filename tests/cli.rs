use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn upyr() -> Command {
    Command::new(env!("CARGO_BIN_EXE_upyr"))
}

fn temporary_config(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir()
        .join(format!("upyr-test-{}-{nonce}", std::process::id()))
        .join(name)
}

#[test]
fn cli_converts_in_both_directions() {
    let english_to_ukrainian = upyr()
        .args(["convert", "ghbdsn"])
        .output()
        .expect("run converter");
    assert!(english_to_ukrainian.status.success());
    assert_eq!(
        String::from_utf8(english_to_ukrainian.stdout).unwrap(),
        "привіт"
    );

    let ukrainian_to_english = upyr()
        .args(["convert", "--direction", "ukrainian-to-english", "руддщ"])
        .output()
        .expect("run converter");
    assert!(ukrainian_to_english.status.success());
    assert_eq!(
        String::from_utf8(ukrainian_to_english.stdout).unwrap(),
        "hello"
    );
}

#[test]
fn init_and_doctor_report_current_configuration() {
    let config = temporary_config("config.toml");
    let initialized = upyr()
        .args(["init"])
        .env("UPYR_CONFIG", &config)
        .output()
        .expect("initialize config");
    assert!(initialized.status.success());

    let source = fs::read_to_string(&config).expect("read initialized config");
    assert!(source.contains("config_version = 12"));
    assert!(source.contains("[globe_switch]"));
    assert!(source.contains("[sounds]"));
    assert!(source.contains("volume_percent = 65"));
    assert!(source.contains("modifier_gesture = \"disabled\""));

    let doctor = upyr()
        .args(["doctor"])
        .env("UPYR_CONFIG", &config)
        .output()
        .expect("run doctor");
    assert!(doctor.status.success());
    let report = String::from_utf8(doctor.stdout).unwrap();
    assert!(report.contains("Config schema: 12"));
    assert!(report.contains("Modifier gesture: Disabled"));
    #[cfg(target_os = "macos")]
    assert!(report.contains("Accessibility access: "));

    let _ = fs::remove_dir_all(config.parent().unwrap());
}

#[cfg(target_os = "macos")]
#[test]
fn globe_configuration_changes_do_not_start_an_agent_or_claim_the_key() {
    let config = temporary_config("globe.toml");
    let enabled = upyr()
        .args(["globe", "enable"])
        .env("UPYR_CONFIG", &config)
        .output()
        .expect("enable integrated switcher");
    assert!(
        enabled.status.success(),
        "{}",
        String::from_utf8_lossy(&enabled.stderr)
    );
    let decoded: upyr::config::Config =
        toml::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
    assert!(decoded.globe_switch.enabled);
    assert!(!decoded.auto_correct);
    let status = upyr()
        .args(["globe", "status"])
        .env("UPYR_CONFIG", &config)
        .output()
        .expect("read switcher status");
    assert!(status.status.success());
    assert!(
        String::from_utf8(status.stdout)
            .unwrap()
            .contains("Globe-key switching: true")
    );
    let listed = upyr()
        .args(["globe", "list"])
        .env("UPYR_CONFIG", &config)
        .output()
        .expect("list input sources");
    assert!(listed.status.success());
    let _ = fs::remove_dir_all(config.parent().unwrap());
}
