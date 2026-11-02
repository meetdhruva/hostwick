use hostwick_core::{HostProfile, ProfileSource};
use serde_json::Value;

use crate::{ImportError, ImportResult, auth_with_key};

/// Best-effort import of a Solar-PuTTY session export saved *without*
/// encryption (Settings → Export, no password). Field names are matched
/// case-insensitively and several spellings are accepted, because the
/// format is undocumented; anything unrecognized becomes a warning.
pub fn parse_solarputty_json(text: &str) -> Result<ImportResult, ImportError> {
    let root: Value =
        serde_json::from_str(text).map_err(|_| ImportError::Format("Solar-PuTTY JSON"))?;
    let sessions = field(&root, &["Sessions"])
        .and_then(Value::as_array)
        .ok_or(ImportError::Format("Solar-PuTTY JSON (no Sessions array)"))?;
    let creds: Vec<&Value> = field(&root, &["Credentials"])
        .and_then(Value::as_array)
        .map(|a| a.iter().collect())
        .unwrap_or_default();

    let mut out = ImportResult::default();
    for s in sessions {
        let name = str_field(s, &["SessionName", "Name"]).unwrap_or_else(|| "imported".into());
        let Some(host) = str_field(s, &["Ip", "Host", "HostName", "Address"]) else {
            out.warnings.push(format!("{name}: skipped (no host)"));
            continue;
        };
        if let Some(kind) = str_field(s, &["ConnectionType", "Protocol"])
            && !kind.eq_ignore_ascii_case("ssh")
            && kind != "0"
        {
            out.warnings
                .push(format!("{name}: skipped (connection type {kind})"));
            continue;
        }
        let cred = str_field(s, &["CredentialsID", "CredentialsId"]).and_then(|id| {
            creds
                .iter()
                .copied()
                .find(|c| str_field(c, &["Id", "ID"]).as_deref() == Some(id.as_str()))
        });
        let mut p = HostProfile::new(name.clone(), host);
        p.port = field(s, &["Port"])
            .and_then(|v| {
                v.as_u64()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            })
            .and_then(|n| u16::try_from(n).ok())
            .unwrap_or(22);
        p.user = cred
            .and_then(|c| str_field(c, &["Username", "UserName", "User"]))
            .or_else(|| str_field(s, &["Username", "UserName"]));
        p.auth = auth_with_key(cred.and_then(|c| str_field(c, &["PrivateKeyPath", "KeyPath"])));
        if cred.and_then(|c| str_field(c, &["Password"])).is_some() {
            out.warnings.push(format!(
                "{name}: saved password not imported; store it in the Hostwick vault"
            ));
        }
        p.source = ProfileSource::SolarPutty { session: name };
        out.profiles.push(p);
    }
    Ok(out)
}

fn field<'a>(v: &'a Value, names: &[&str]) -> Option<&'a Value> {
    let obj = v.as_object()?;
    names.iter().find_map(|n| {
        obj.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(n))
            .map(|(_, v)| v)
    })
}

fn str_field(v: &Value, names: &[&str]) -> Option<String> {
    match field(v, names)? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sessions_with_credentials() {
        let json = r#"{
          "Sessions": [
            {"Id":"s1","SessionName":"web","Ip":"10.0.0.5","Port":2200,"CredentialsID":"c1","ConnectionType":0},
            {"Id":"s2","SessionName":"rdp-box","Ip":"10.0.0.6","ConnectionType":"RDP"},
            {"Id":"s3","SessionName":"nohost"}
          ],
          "Credentials": [
            {"Id":"c1","CredentialsName":"deploy","Username":"deploy","PrivateKeyPath":"C:\\keys\\id","Password":"x"}
          ]
        }"#;
        let r = parse_solarputty_json(json).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r.profiles.len(), 1);
        let p = &r.profiles[0];
        assert_eq!(
            (p.host.as_str(), p.port, p.user.as_deref()),
            ("10.0.0.5", 2200, Some("deploy"))
        );
        assert_eq!(p.auth.len(), 4);
        assert_eq!(r.warnings.len(), 3, "{:?}", r.warnings);
        assert!(parse_solarputty_json("not json").is_err());
        assert!(parse_solarputty_json("{}").is_err());
    }
}
