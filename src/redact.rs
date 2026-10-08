use regex::Regex;
use std::sync::OnceLock;

static PRIVATE_KEY_REGEX: OnceLock<Regex> = OnceLock::new();
static AWS_ACCESS_KEY_REGEX: OnceLock<Regex> = OnceLock::new();
static GITHUB_TOKEN_REGEX: OnceLock<Regex> = OnceLock::new();
static GITLAB_TOKEN_REGEX: OnceLock<Regex> = OnceLock::new();
static SLACK_TOKEN_REGEX: OnceLock<Regex> = OnceLock::new();
static STRIPE_KEY_REGEX: OnceLock<Regex> = OnceLock::new();
static AI_KEY_REGEX: OnceLock<Regex> = OnceLock::new();
static GOOGLE_KEY_REGEX: OnceLock<Regex> = OnceLock::new();
static PYPI_TOKEN_REGEX: OnceLock<Regex> = OnceLock::new();
static NPM_TOKEN_REGEX: OnceLock<Regex> = OnceLock::new();
static HUGGINGFACE_TOKEN_REGEX: OnceLock<Regex> = OnceLock::new();
static SENDGRID_KEY_REGEX: OnceLock<Regex> = OnceLock::new();
static JWT_REGEX: OnceLock<Regex> = OnceLock::new();
static AUTH_BEARER_REGEX: OnceLock<Regex> = OnceLock::new();
static URL_CREDENTIALS_REGEX: OnceLock<Regex> = OnceLock::new();
static KV_DOUBLE_QUOTE_REGEX: OnceLock<Regex> = OnceLock::new();
static KV_SINGLE_QUOTE_REGEX: OnceLock<Regex> = OnceLock::new();
static KV_UNQUOTED_REGEX: OnceLock<Regex> = OnceLock::new();

fn is_sensitive_value(val: &str) -> bool {
    let trimmed = val.trim();
    if trimmed.is_empty() || trimmed.starts_with("[REDACTED") {
        return false;
    }
    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let lower = trimmed.to_lowercase();
    if matches!(
        lower.as_str(),
        "true"
            | "false"
            | "null"
            | "none"
            | "nil"
            | "undefined"
            | "default"
            | "test"
            | "testing"
            | "example"
            | "sample"
            | "dummy"
            | "placeholder"
            | "string"
            | "str"
            | "bool"
            | "boolean"
    ) {
        return false;
    }
    trimmed.len() >= 4
}

/// Redact sensitive secrets, tokens, keys, and passwords from text
pub fn redact_secrets(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }

    let priv_key_re = PRIVATE_KEY_REGEX.get_or_init(|| {
        Regex::new(r"-----BEGIN (?:[A-Z0-9_-]+ )?PRIVATE KEY(?: BLOCK)?-----[	
\r -~]*?-----END (?:[A-Z0-9_-]+ )?PRIVATE KEY(?: BLOCK)?-----")
            .expect("Failed to compile private key regex")
    });
    let result = priv_key_re.replace_all(text, "[REDACTED PRIVATE KEY]");

    let aws_re = AWS_ACCESS_KEY_REGEX.get_or_init(|| {
        Regex::new(r"\b(AKIA|ASIA|AROA|AIPA)[0-9A-Z]{16}\b")
            .expect("Failed to compile AWS key regex")
    });
    let result = aws_re.replace_all(&result, "[REDACTED AWS KEY]");

    let gh_re = GITHUB_TOKEN_REGEX.get_or_init(|| {
        Regex::new(r"\b(?:(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9_]{36,}|github_pat_[A-Za-z0-9_]{82,})\b")
            .expect("Failed to compile GitHub token regex")
    });
    let result = gh_re.replace_all(&result, "[REDACTED GITHUB TOKEN]");

    let gl_re = GITLAB_TOKEN_REGEX.get_or_init(|| {
        Regex::new(r"\b(?:glpat|gldt)-[A-Za-z0-9_-]{20,}\b")
            .expect("Failed to compile GitLab token regex")
    });
    let result = gl_re.replace_all(&result, "[REDACTED GITLAB TOKEN]");

    let slack_re = SLACK_TOKEN_REGEX.get_or_init(|| {
        Regex::new(r"\bxox[baprs]-[0-9]{10,13}-[0-9]{10,13}[a-zA-Z0-9-]*\b")
            .expect("Failed to compile Slack token regex")
    });
    let result = slack_re.replace_all(&result, "[REDACTED SLACK TOKEN]");

    let stripe_re = STRIPE_KEY_REGEX.get_or_init(|| {
        Regex::new(r"\b(?:sk|pk|rk)_(?:live|test)_[0-9a-zA-Z]{24,}\b")
            .expect("Failed to compile Stripe key regex")
    });
    let result = stripe_re.replace_all(&result, "[REDACTED STRIPE KEY]");

    let ai_re = AI_KEY_REGEX.get_or_init(|| {
        Regex::new(r"\b(?:sk-(?:proj-|svcacct-)?[A-Za-z0-9_-]{32,}|sk-ant-[A-Za-z0-9_-]{32,})\b")
            .expect("Failed to compile AI key regex")
    });
    let result = ai_re.replace_all(&result, "[REDACTED API KEY]");

    let google_re = GOOGLE_KEY_REGEX.get_or_init(|| {
        Regex::new(r"\bAIza[0-9A-Za-z-_]{35}\b")
            .expect("Failed to compile Google key regex")
    });
    let result = google_re.replace_all(&result, "[REDACTED GOOGLE KEY]");

    let pypi_re = PYPI_TOKEN_REGEX.get_or_init(|| {
        Regex::new(r"\bpypi-AgEIcHlwaS5vcmc[A-Za-z0-9-_]{50,}\b")
            .expect("Failed to compile PyPI token regex")
    });
    let result = pypi_re.replace_all(&result, "[REDACTED PYPI TOKEN]");

    let npm_re = NPM_TOKEN_REGEX.get_or_init(|| {
        Regex::new(r"\bnpm_[A-Za-z0-9]{36,}\b")
            .expect("Failed to compile NPM token regex")
    });
    let result = npm_re.replace_all(&result, "[REDACTED NPM TOKEN]");

    let hf_re = HUGGINGFACE_TOKEN_REGEX.get_or_init(|| {
        Regex::new(r"\bhf_[A-Za-z0-9]{34,}\b")
            .expect("Failed to compile Hugging Face token regex")
    });
    let result = hf_re.replace_all(&result, "[REDACTED HUGGINGFACE TOKEN]");

    let sg_re = SENDGRID_KEY_REGEX.get_or_init(|| {
        Regex::new(r"\bSG\.[A-Za-z0-9_-]{22}\.[A-Za-z0-9_-]{43}\b")
            .expect("Failed to compile SendGrid key regex")
    });
    let result = sg_re.replace_all(&result, "[REDACTED SENDGRID KEY]");

    let jwt_re = JWT_REGEX.get_or_init(|| {
        Regex::new(r"\beyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]+\b")
            .expect("Failed to compile JWT regex")
    });
    let result = jwt_re.replace_all(&result, "[REDACTED JWT]");

    let bearer_re = AUTH_BEARER_REGEX.get_or_init(|| {
        Regex::new(r"(?i)\b(bearer|token)\s+([A-Za-z0-9_\-\.~+/]{20,}=*)\b")
            .expect("Failed to compile Auth Bearer regex")
    });
    let result = bearer_re.replace_all(&result, |caps: &regex::Captures| {
        let token = &caps[2];
        if token.starts_with("[REDACTED") {
            caps[0].to_string()
        } else {
            format!("{} [REDACTED]", &caps[1])
        }
    });

    let url_re = URL_CREDENTIALS_REGEX.get_or_init(|| {
        Regex::new(r"(?i)([a-zA-Z][a-zA-Z0-9+.-]*://[^:\s/@]+):([^@\s/]{3,})@")
            .expect("Failed to compile URL credentials regex")
    });
    let result = url_re.replace_all(&result, |caps: &regex::Captures| {
        let pass = &caps[2];
        if pass.starts_with("[REDACTED") {
            caps[0].to_string()
        } else {
            format!("{}:[REDACTED]@", &caps[1])
        }
    });

    let kv_dq_re = KV_DOUBLE_QUOTE_REGEX.get_or_init(|| {
        Regex::new(r#"(?i)(["']?(?:password|passwd|pwd|secret_key|api_key|apikey|api_secret|access_token|auth_token|client_secret|private_key|bearer_token|session_token|auth_secret|encryption_key|master_key|secret)["']?\s*[:=]\s*)(")([^"\r\n]+)(")"#)
            .expect("Failed to compile KV double quote regex")
    });
    let result = kv_dq_re.replace_all(&result, |caps: &regex::Captures| {
        let prefix = &caps[1];
        let val = &caps[3];
        if is_sensitive_value(val) {
            format!("{}\"[REDACTED]\"", prefix)
        } else {
            caps[0].to_string()
        }
    });

    let kv_sq_re = KV_SINGLE_QUOTE_REGEX.get_or_init(|| {
        Regex::new(r#"(?i)(["']?(?:password|passwd|pwd|secret_key|api_key|apikey|api_secret|access_token|auth_token|client_secret|private_key|bearer_token|session_token|auth_secret|encryption_key|master_key|secret)["']?\s*[:=]\s*)(')([^'\r\n]+)(')"#)
            .expect("Failed to compile KV single quote regex")
    });
    let result = kv_sq_re.replace_all(&result, |caps: &regex::Captures| {
        let prefix = &caps[1];
        let val = &caps[3];
        if is_sensitive_value(val) {
            format!("{}'[REDACTED]'", prefix)
        } else {
            caps[0].to_string()
        }
    });

    let kv_unq_re = KV_UNQUOTED_REGEX.get_or_init(|| {
        Regex::new(r#"(?i)(["']?(?:password|passwd|pwd|secret_key|api_key|apikey|api_secret|access_token|auth_token|client_secret|private_key|bearer_token|session_token|auth_secret|encryption_key|master_key|secret)["']?\s*[:=]\s*)([^\s"'#;,\r\n]+)"#)
            .expect("Failed to compile KV unquoted regex")
    });
    let result = kv_unq_re.replace_all(&result, |caps: &regex::Captures| {
        let prefix = &caps[1];
        let val = &caps[2];
        if is_sensitive_value(val) {
            format!("{}[REDACTED]", prefix)
        } else {
            caps[0].to_string()
        }
    });

    result.into_owned()
}
