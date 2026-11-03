//! Profile name validation and read-only parsing of `azureProfile.json`.
//!
//! Only `azureProfile.json` is ever read. Token caches (`msal_token_cache.*`,
//! `accessTokens.json`, `service_principal_entries.*`) are never opened.

use anyhow::{bail, Context, Result};
use serde_json::Value;

/// File written by `az` that lists the subscriptions available to the login.
pub const AZURE_PROFILE_FILE: &str = "azureProfile.json";

/// Accept only non-empty names made of `[a-z0-9_-]`.
pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("profile name must not be empty");
    }
    if let Some(c) = name
        .chars()
        .find(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_' || *c == '-'))
    {
        bail!("invalid character {c:?} in profile name {name:?}: only [a-z0-9_-] is allowed");
    }
    Ok(())
}

/// The bits of `azureProfile.json` shown by `azp list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileInfo {
    pub user: String,
    pub tenant: String,
    pub subscription: String,
}

/// Parse the contents of `azureProfile.json`.
///
/// Returns `Ok(None)` when the file has no subscriptions, which is what `az`
/// leaves behind after `az logout`. The default subscription is the one with
/// `isDefault: true`, falling back to the first entry.
pub fn parse_azure_profile(contents: &str) -> Result<Option<ProfileInfo>> {
    // az writes this file as UTF-8 with a BOM.
    let contents = contents.strip_prefix('\u{feff}').unwrap_or(contents);
    let root: Value = serde_json::from_str(contents).context("invalid azureProfile.json")?;

    let subs = match root.get("subscriptions").and_then(Value::as_array) {
        Some(subs) if !subs.is_empty() => subs,
        _ => return Ok(None),
    };
    let sub = subs
        .iter()
        .find(|s| s.get("isDefault").and_then(Value::as_bool) == Some(true))
        .unwrap_or(&subs[0]);

    let str_at = |v: &Value, keys: &[&str]| -> String {
        keys.iter()
            .find_map(|k| v.pointer(k).and_then(Value::as_str).filter(|s| !s.is_empty()))
            .unwrap_or("-")
            .to_string()
    };

    Ok(Some(ProfileInfo {
        user: str_at(sub, &["/user/name"]),
        tenant: str_at(sub, &["/tenantDefaultDomain", "/tenantDisplayName", "/tenantId"]),
        subscription: str_at(sub, &["/name", "/id"]),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_names() {
        for name in ["work", "client-a", "prod_eu", "a", "0", "my-sub_2"] {
            assert!(validate_name(name).is_ok(), "{name} should be valid");
        }
    }

    #[test]
    fn invalid_names() {
        for name in [
            "", "Work", "a b", "a/b", "..", ".", "a.b", "../etc", "é", "x\n", "ü", "a$b", "~",
        ] {
            assert!(validate_name(name).is_err(), "{name:?} should be invalid");
        }
    }

    const SAMPLE: &str = r#"{
        "installationId": "00000000-0000-0000-0000-000000000000",
        "subscriptions": [
            {
                "id": "11111111-1111-1111-1111-111111111111",
                "name": "Dev",
                "state": "Enabled",
                "user": { "name": "dev@contoso.com", "type": "user" },
                "isDefault": false,
                "tenantId": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
                "tenantDefaultDomain": "contoso.onmicrosoft.com",
                "environmentName": "AzureCloud"
            },
            {
                "id": "22222222-2222-2222-2222-222222222222",
                "name": "Prod",
                "state": "Enabled",
                "user": { "name": "me@contoso.com", "type": "user" },
                "isDefault": true,
                "tenantId": "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
                "tenantDefaultDomain": "prod.onmicrosoft.com",
                "environmentName": "AzureCloud"
            }
        ]
    }"#;

    #[test]
    fn picks_default_subscription() {
        let info = parse_azure_profile(SAMPLE).unwrap().unwrap();
        assert_eq!(
            info,
            ProfileInfo {
                user: "me@contoso.com".into(),
                tenant: "prod.onmicrosoft.com".into(),
                subscription: "Prod".into(),
            }
        );
    }

    #[test]
    fn handles_bom() {
        let with_bom = format!("\u{feff}{SAMPLE}");
        assert!(parse_azure_profile(&with_bom).unwrap().is_some());
    }

    #[test]
    fn falls_back_to_first_subscription_and_tenant_id() {
        let json = r#"{"subscriptions":[
            {"id":"s1","name":"First","user":{"name":"u@x"},"tenantId":"t1"},
            {"id":"s2","name":"Second","user":{"name":"v@x"},"tenantId":"t2"}
        ]}"#;
        let info = parse_azure_profile(json).unwrap().unwrap();
        assert_eq!(info.subscription, "First");
        assert_eq!(info.user, "u@x");
        assert_eq!(info.tenant, "t1");
    }

    #[test]
    fn missing_fields_render_as_dash() {
        let info = parse_azure_profile(r#"{"subscriptions":[{}]}"#).unwrap().unwrap();
        assert_eq!(info.user, "-");
        assert_eq!(info.tenant, "-");
        assert_eq!(info.subscription, "-");
    }

    #[test]
    fn empty_subscriptions_means_logged_out() {
        assert_eq!(parse_azure_profile(r#"{"subscriptions":[]}"#).unwrap(), None);
        assert_eq!(parse_azure_profile(r#"{"installationId":"x"}"#).unwrap(), None);
    }

    #[test]
    fn invalid_json_is_an_error() {
        assert!(parse_azure_profile("not json").is_err());
    }
}
