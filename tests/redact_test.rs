use tempfile::tempdir;
use yek::{config::YekConfig, redact::redact_secrets, serialize_repo};

#[test]
fn test_redact_private_key() {
    let input = "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEAz...\n-----END RSA PRIVATE KEY-----";
    let redacted = redact_secrets(input);
    assert_eq!(redacted, "[REDACTED PRIVATE KEY]");
}

#[test]
fn test_redact_aws_key() {
    let input = "const awsKey = 'AKIAIOSFODNN7EXAMPLE';";
    let redacted = redact_secrets(input);
    assert_eq!(redacted, "const awsKey = '[REDACTED AWS KEY]';");
}

#[test]
fn test_redact_github_token() {
    let input = "GH_TOKEN=ghp_123456789012345678901234567890123456";
    let redacted = redact_secrets(input);
    assert!(redacted.contains("[REDACTED GITHUB TOKEN]"));
    assert!(!redacted.contains("ghp_123456789012345678901234567890123456"));
}

#[test]
fn test_redact_slack_token() {
    let input = "slack = 'xoxb-123456789012-1234567890123-abcdefghijklmnopqrstuvwx';";
    let redacted = redact_secrets(input);
    assert_eq!(redacted, "slack = '[REDACTED SLACK TOKEN]';");
}

#[test]
fn test_redact_stripe_key() {
    let input = "sk_live_51Abcdefghijklmnopqrstuvw";
    let redacted = redact_secrets(input);
    assert_eq!(redacted, "[REDACTED STRIPE KEY]");
}

#[test]
fn test_redact_ai_api_key() {
    let input = "OPENAI_API_KEY=sk-proj-1234567890abcdef1234567890abcdef";
    let redacted = redact_secrets(input);
    assert!(redacted.contains("[REDACTED API KEY]"));
    assert!(!redacted.contains("sk-proj-"));
}

#[test]
fn test_redact_jwt() {
    let input = "token: eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
    let redacted = redact_secrets(input);
    assert_eq!(redacted, "token: [REDACTED JWT]");
}

#[test]
fn test_redact_bearer_token() {
    let input = "Authorization: Bearer 1234567890abcdef1234567890";
    let redacted = redact_secrets(input);
    assert_eq!(redacted, "Authorization: Bearer [REDACTED]");
}

#[test]
fn test_redact_url_credentials() {
    let input = "postgres://myuser:supersecretpass@db.internal:5432/mydb";
    let redacted = redact_secrets(input);
    assert_eq!(
        redacted,
        "postgres://myuser:[REDACTED]@db.internal:5432/mydb"
    );
}

#[test]
fn test_redact_kv_passwords() {
    let input = r#"{
  "password": "super_secret_value_123",
  "api_key": 'my_secret_api_key_456',
  "client_secret": custom_secret_789
}"#;
    let redacted = redact_secrets(input);
    assert!(redacted.contains(r#""password": "[REDACTED]""#));
    assert!(redacted.contains(r#""api_key": '[REDACTED]'"#));
    assert!(redacted.contains(r#""client_secret": [REDACTED]"#));
    assert!(!redacted.contains("super_secret_value_123"));
    assert!(!redacted.contains("my_secret_api_key_456"));
    assert!(!redacted.contains("custom_secret_789"));
}

#[test]
fn test_preserve_non_sensitive_values() {
    let input = r#"{
  "password": true,
  "secret": false,
  "timeout": 30,
  "tokens": 1000,
  "max_tokens": "2000",
  "dummy": "test"
}"#;
    let redacted = redact_secrets(input);
    assert!(redacted.contains(r#""password": true"#));
    assert!(redacted.contains(r#""secret": false"#));
    assert!(redacted.contains(r#""tokens": 1000"#));
}

#[test]
fn test_redaction_idempotence() {
    let input = "password = \"my_secret_value\"";
    let once = redact_secrets(input);
    let twice = redact_secrets(&once);
    assert_eq!(once, "password = \"[REDACTED]\"");
    assert_eq!(once, twice);
}

#[test]
fn test_serialize_repo_redacts_tokens_by_default() {
    let temp_dir = tempdir().unwrap();
    let file_path = temp_dir.path().join("config.env");
    std::fs::write(
        &file_path,
        "API_KEY=ghp_123456789012345678901234567890123456\nPASSWORD=super_secret_pw\n",
    )
    .unwrap();

    let config = YekConfig {
        input_paths: vec![temp_dir.path().to_string_lossy().to_string()],
        stream: true,
        ..YekConfig::default()
    };

    let (output, _) = serialize_repo(&config).unwrap();
    assert!(!output.contains("ghp_123456789012345678901234567890123456"));
    assert!(!output.contains("super_secret_pw"));
    assert!(output.contains("[REDACTED"));
}

#[test]
fn test_serialize_repo_no_redact_flag() {
    let temp_dir = tempdir().unwrap();
    let file_path = temp_dir.path().join("config.env");
    std::fs::write(&file_path, "PASSWORD=super_secret_pw\n").unwrap();

    let config = YekConfig {
        input_paths: vec![temp_dir.path().to_string_lossy().to_string()],
        stream: true,
        no_redact: true,
        redact: false,
        ..YekConfig::default()
    };

    let (output, _) = serialize_repo(&config).unwrap();
    assert!(output.contains("PASSWORD=super_secret_pw"));
}
