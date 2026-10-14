//! Direct native GitHub App operations. User credentials stay outside the WebView.
use crate::github_http::{self, Authentication, Request};
use reqwest::Method;
use serde_json::{json, Map, Value};

pub struct Configuration {
    pub client_id: String,
    pub client_secret: String,
}
#[derive(Clone, Copy)]
pub enum Operation {
    Exchange,
    Refresh,
    Scope,
    RevokeToken,
    RevokeAuthorization,
}
const INVALID: &str = "Invalid GitHub authorization request.";
const REPOSITORY_PERMISSIONS: &[&str] = &[
    "actions",
    "administration",
    "artifact_metadata",
    "attestations",
    "checks",
    "code_quality",
    "codespaces",
    "codespaces_lifecycle_admin",
    "codespaces_metadata",
    "codespaces_secrets",
    "contents",
    "dependabot_secrets",
    "deployments",
    "discussions",
    "environments",
    "issues",
    "merge_queues",
    "metadata",
    "packages",
    "pages",
    "pull_requests",
    "repository_custom_properties",
    "repository_hooks",
    "repository_projects",
    "repository_advisories",
    "secret_scanning_alerts",
    "secrets",
    "security_events",
    "single_file",
    "statuses",
    "vulnerability_alerts",
    "workflows",
    "actions_variables",
];
// Token permissions are explicit allowlists, each capped by what the App
// installation grants (docs/SiloUI-REVIEW-DESIGN-NOTES.md, B-05). Admin and
// secret-bearing scopes are never requested.
const COMPUTER_READ: &[(&str, &str)] = &[
    ("metadata", "read"),
    ("contents", "read"),
    ("issues", "read"),
    ("pull_requests", "read"),
    ("statuses", "read"),
    ("checks", "read"),
];
/// "Allow GitHub changes" also lets agents change workflows and read CI runs.
const COMPUTER_WRITE: &[(&str, &str)] = &[
    ("metadata", "read"),
    ("contents", "write"),
    ("issues", "write"),
    ("pull_requests", "write"),
    ("statuses", "write"),
    ("checks", "read"),
    ("workflows", "write"),
    ("actions", "read"),
];
/// Host push publishes one branch of one repository (owner decision 1).
const HOST_PUSH: &[(&str, &str)] = &[("metadata", "read"), ("contents", "write")];
fn rank(level: &str) -> u8 {
    match level {
        "read" => 1,
        "write" => 2,
        "admin" => 3,
        _ => 0,
    }
}
fn allowed(installation: &Map<String, Value>, allowlist: &[(&str, &str)]) -> Map<String, Value> {
    let mut selected = Map::new();
    for (name, wanted) in allowlist {
        // Every installation can read repository metadata.
        let granted = if *name == "metadata" {
            Some("read")
        } else {
            installation.get(*name).and_then(Value::as_str)
        };
        if let Some(granted) = granted {
            let level = if rank(granted) >= rank(wanted) {
                *wanted
            } else {
                granted
            };
            selected.insert((*name).into(), json!(level));
        }
    }
    selected
}
fn text(value: &Value) -> Result<&str, String> {
    value
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 1024 && !v.bytes().any(|b| b <= 32 || b == 127))
        .ok_or_else(|| INVALID.into())
}
fn id(value: &Value) -> Result<u64, String> {
    value
        .as_u64()
        .filter(|v| *v > 0 && *v <= 9_007_199_254_740_991)
        .ok_or_else(|| INVALID.into())
}
fn session(value: Value) -> Result<Value, String> {
    Ok(
        json!({"accessToken":text(&value["access_token"])?, "refreshToken":text(&value["refresh_token"])?,
        "expiresIn":id(&value["expires_in"])?, "refreshTokenExpiresIn":id(&value["refresh_token_expires_in"])?}),
    )
}
pub fn execute(config: &Configuration, operation: Operation, body: Value) -> Result<Value, String> {
    execute_with(config, operation, body, github_http::send)
}
pub(crate) fn execute_for_computer(
    config: &Configuration,
    operation: Operation,
    body: Value,
    computer: &str,
) -> Result<Value, String> {
    execute_with(config, operation, body, |request| {
        github_http::send_for_computer(request, computer)
    })
}
fn execute_with(
    config: &Configuration,
    operation: Operation,
    input: Value,
    mut send: impl FnMut(Request) -> Result<Value, String>,
) -> Result<Value, String> {
    if config.client_id.is_empty()
        || matches!(config.client_id.as_str(), "." | "..")
        || !config
            .client_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.')
        || config.client_secret.is_empty()
        || config.client_secret.len() > 1024
        || config.client_secret.bytes().any(|b| b <= 32 || b == 127)
        || !input.is_object()
    {
        return Err(INVALID.into());
    }
    let app = || Authentication::App {
        client_id: config.client_id.clone(),
        client_secret: config.client_secret.clone(),
    };
    let oauth = |body| Request {
        method: Method::POST,
        url: "https://github.com/login/oauth/access_token".into(),
        authentication: Authentication::None,
        body,
        safe: false,
        revoke: false,
    };
    match operation {
        Operation::Exchange => {
            let code = text(&input["code"])?;
            let verifier = text(&input["codeVerifier"])?;
            if !(43..=128).contains(&verifier.len())
                || !verifier
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._~-".contains(&b))
            {
                return Err(INVALID.into());
            }
            let redirect = text(&input["redirectUri"])?;
            let url = reqwest::Url::parse(redirect).map_err(|_| INVALID)?;
            if url.scheme() != "http"
                || url.host_str() != Some("127.0.0.1")
                || url.port().unwrap_or(0) < 1024
                || !url.username().is_empty()
                || url.password().is_some()
                || url.path() != "/github/callback"
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(INVALID.into());
            }
            let response = send(oauth(
                json!({"client_id":config.client_id,"client_secret":config.client_secret,"code":code,"code_verifier":verifier,"redirect_uri":redirect}),
            ))?;
            let result = session(response.clone());
            if result.is_err() {
                // The exchange may issue a non-expiring token that Silo cannot
                // keep. Discard only this token; a previous login may share its grant.
                if let Ok(token) = text(&response["access_token"]) {
                    let _ = send(Request {
                        method: Method::DELETE,
                        url: format!(
                            "https://api.github.com/applications/{}/token",
                            config.client_id
                        ),
                        authentication: app(),
                        body: json!({"access_token":token}),
                        safe: true,
                        revoke: true,
                    });
                }
            }
            result
        }
        Operation::Refresh => session(send(oauth(
            json!({"client_id":config.client_id,"client_secret":config.client_secret,"grant_type":"refresh_token","refresh_token":text(&input["refreshToken"])?}),
        ))?),
        Operation::RevokeToken | Operation::RevokeAuthorization => {
            let target = if matches!(operation, Operation::RevokeToken) {
                "token"
            } else {
                "grant"
            };
            send(Request {
                method: Method::DELETE,
                url: format!(
                    "https://api.github.com/applications/{}/{target}",
                    config.client_id
                ),
                authentication: app(),
                body: json!({"access_token":text(&input["accessToken"])?}),
                safe: true,
                revoke: true,
            })?;
            Ok(json!({"revoked":true}))
        }
        Operation::Scope => {
            let token = text(&input["accessToken"])?;
            let owner = id(&input["ownerId"])?;
            let changes = input["allowChanges"].as_bool().ok_or(INVALID)?;
            let host_push = match input.get("purpose") {
                None => false,
                Some(purpose) if purpose == "hostPush" => true,
                Some(_) => return Err(INVALID.into()),
            };
            let all = input
                .get("allRepositories")
                .map(|v| v.as_bool().ok_or(INVALID))
                .transpose()?
                .unwrap_or(false);
            let repositories = input["repositoryIds"].as_array().ok_or(INVALID)?;
            if repositories.len() > 500
                || (all && !repositories.is_empty())
                || (!all && repositories.is_empty())
                || (host_push && (all || repositories.len() != 1))
            {
                return Err(INVALID.into());
            }
            let allowlist = if host_push {
                HOST_PUSH
            } else if changes {
                COMPUTER_WRITE
            } else {
                COMPUTER_READ
            };
            let ids: Vec<u64> = repositories.iter().map(id).collect::<Result<_, _>>()?;
            if ids.iter().collect::<std::collections::HashSet<_>>().len() != ids.len() {
                return Err(INVALID.into());
            }
            let mut permissions = None;
            for page in 1..=20 {
                let result = send(Request {
                    method: Method::GET,
                    url: format!(
                        "https://api.github.com/user/installations?per_page=100&page={page}"
                    ),
                    authentication: Authentication::Bearer(token.into()),
                    body: Value::Null,
                    safe: true,
                    revoke: false,
                })?;
                let installations = result["installations"]
                    .as_array()
                    .ok_or("GitHub returned an invalid installation list.")?;
                for installation in installations {
                    // GitHub lists only this token's App; client_id is optional.
                    if installation["account"]["id"].as_u64() != Some(owner)
                        || installation.get("client_id").is_some_and(|client_id| {
                            client_id.as_str() != Some(config.client_id.as_str())
                        })
                        || !installation["suspended_at"].is_null()
                    {
                        continue;
                    }
                    let granted = installation["permissions"]
                        .as_object()
                        .ok_or("GitHub returned invalid App permissions.")?;
                    for (name, level) in granted {
                        if !REPOSITORY_PERMISSIONS.contains(&name.as_str()) {
                            return Err("The GitHub App has unsupported or non-repository permissions. Its administrator must remove them before Silo can grant computer access.".into());
                        }
                        if !matches!(level.as_str(), Some("read" | "write" | "admin")) {
                            return Err("GitHub returned invalid App permissions.".into());
                        }
                    }
                    let selected = allowed(granted, allowlist);
                    if host_push && selected.get("contents") != Some(&json!("write")) {
                        return Err("The GitHub App cannot push commits. Its administrator must grant Contents write access.".into());
                    }
                    permissions = Some(selected);
                    break;
                }
                if permissions.is_some() || installations.len() < 100 {
                    break;
                }
                if page == 20 {
                    return Err("Too many GitHub installations to resolve safely.".into());
                }
            }
            let permissions = permissions.ok_or("This GitHub owner is not authorized for Silo.")?;
            let mut body =
                json!({"access_token":token,"target_id":owner,"permissions":permissions});
            if !all {
                body["repository_ids"] = json!(ids);
            }
            let result = send(Request {
                method: Method::POST,
                url: format!(
                    "https://api.github.com/applications/{}/token/scoped",
                    config.client_id
                ),
                authentication: app(),
                body,
                safe: false,
                revoke: false,
            })?;
            let expires = text(&result["expires_at"])?;
            let parsed = time::OffsetDateTime::parse(
                expires,
                &time::format_description::well_known::Rfc3339,
            )
            .map_err(|_| "GitHub returned an invalid token expiration.")?;
            if parsed <= time::OffsetDateTime::now_utc() {
                return Err("GitHub returned an expired token.".into());
            }
            Ok(json!({"accessToken":text(&result["token"])?,"expiresAt":expires}))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> Configuration {
        Configuration {
            client_id: "Iv23test".into(),
            client_secret: "fixture-secret".into(),
        }
    }
    fn input() -> Value {
        json!({"accessToken":"parent-fixture","ownerId":7,"repositoryIds":[11,12],"allowChanges":false})
    }
    fn installation(permissions: Value) -> Value {
        json!({"account":{"id":7},"client_id":"Iv23test","suspended_at":null,"permissions":permissions})
    }
    fn scoped() -> Value {
        json!({"token":"scoped-fixture","expires_at":"2099-01-01T00:00:00Z"})
    }
    #[test]
    fn code_exchange_carries_pkce_directly_and_requires_rotating_session() {
        for client_id in ["Iv23test", "Iv1.ab1112223334445c"] {
            let config = Configuration {
                client_id: client_id.into(),
                ..config()
            };
            let result = execute_with(
                &config,
                Operation::Exchange,
                json!({"code":"fixture-code","codeVerifier":"a".repeat(43),"redirectUri":"http://127.0.0.1:4321/github/callback"}),
                |r| {
                    assert_eq!(r.url, "https://github.com/login/oauth/access_token");
                    assert!(!r.safe);
                    assert_eq!(r.body["client_id"], client_id);
                    assert_eq!(r.body["code_verifier"], "a".repeat(43));
                    assert_eq!(r.body["client_secret"], "fixture-secret");
                    Ok(json!({"access_token":"access","refresh_token":"refresh","expires_in":28800,"refresh_token_expires_in":15811200}))
                },
            )
            .unwrap();
            assert_eq!(result["accessToken"], "access");
        }
        assert!(session(json!({"access_token":"access"})).is_err());
    }
    #[test]
    fn rejected_exchange_revokes_its_unstored_token_without_revoking_the_grant() {
        for response in [
            json!({"access_token":"non-expiring-fixture","token_type":"bearer","scope":""}),
            json!({"access_token":"invalid-session-fixture","refresh_token":"refresh","expires_in":0,"refresh_token_expires_in":15897600}),
        ] {
            let token = response["access_token"].as_str().unwrap().to_owned();
            let mut calls = 0;
            let result = execute_with(
                &config(),
                Operation::Exchange,
                json!({"code":"fixture-code","codeVerifier":"a".repeat(43),"redirectUri":"http://127.0.0.1:4321/github/callback"}),
                |request| {
                    calls += 1;
                    if calls == 1 {
                        assert_eq!(request.method, Method::POST);
                        return Ok(response.clone());
                    }
                    assert_eq!(request.method, Method::DELETE);
                    assert_eq!(
                        request.url,
                        "https://api.github.com/applications/Iv23test/token"
                    );
                    assert_eq!(request.body, json!({"access_token":token}));
                    assert!(matches!(request.authentication, Authentication::App { .. }));
                    assert!(request.safe && request.revoke);
                    Ok(json!({"revoked":true}))
                },
            );
            assert!(result.is_err());
            assert_eq!(
                calls, 2,
                "Rejected exchange left its issued token unrevoked"
            );
        }
        let mut calls = 0;
        assert!(execute_with(
            &config(), Operation::Exchange,
            json!({"code":"fixture-code","codeVerifier":"a".repeat(43),"redirectUri":"http://127.0.0.1:4321/github/callback"}),
            |_| { calls += 1; Ok(json!({})) },
        ).is_err());
        assert_eq!(calls, 1);
    }
    #[test]
    fn callback_and_verifier_are_validated_before_network() {
        for redirect in [
            "https://127.0.0.1:4321/github/callback",
            "http://localhost:4321/github/callback",
            "http://127.0.0.1/github/callback",
            "http://127.0.0.1:4321/other",
            "http://127.0.0.1:4321/github/callback?x=1",
        ] {
            assert!(execute_with(
                &config(),
                Operation::Exchange,
                json!({"code":"code","codeVerifier":"a".repeat(43),"redirectUri":redirect}),
                |_| panic!("unexpected network")
            )
            .is_err());
        }
        assert!(execute_with(&config(),Operation::Exchange,json!({"code":"code","codeVerifier":"short","redirectUri":"http://127.0.0.1:4321/github/callback"}), |_|panic!("unexpected network")).is_err());
    }
    #[test]
    fn refresh_and_each_revocation_have_distinct_contracts() {
        for client_id in ["Iv23test", "Iv1.ab1112223334445c"] {
            let config = Configuration {
                client_id: client_id.into(),
                ..config()
            };
            execute_with(
                &config,
                Operation::Refresh,
                json!({"refreshToken":"refresh"}),
                |r| {
                    assert_eq!(r.body["client_id"], client_id);
                    assert_eq!(r.body["grant_type"], "refresh_token");
                    assert!(!r.safe);
                    Ok(json!({"access_token":"access","refresh_token":"new-refresh","expires_in":1,"refresh_token_expires_in":2}))
                },
            )
            .unwrap();
            for (operation, path) in [
                (Operation::RevokeToken, "token"),
                (Operation::RevokeAuthorization, "grant"),
            ] {
                execute_with(&config, operation, json!({"accessToken":"access"}), |r| {
                    assert_eq!(r.method, Method::DELETE);
                    assert!(r.safe && r.revoke);
                    assert_eq!(
                        r.url,
                        format!("https://api.github.com/applications/{client_id}/{path}")
                    );
                    assert_eq!(r.body["access_token"], "access");
                    match r.authentication {
                        Authentication::App {
                            client_id: sent_client_id,
                            client_secret,
                        } => {
                            assert_eq!(sent_client_id, client_id);
                            assert_eq!(client_secret, "fixture-secret");
                        }
                        _ => panic!("App authentication required"),
                    };
                    Ok(json!({"revoked":true}))
                })
                .unwrap();
            }
        }
    }
    #[test]
    fn scoped_read_omits_write_only_authority_and_binds_owner_and_repositories() {
        for client_id in ["Iv23test", "Iv1.ab1112223334445c"] {
            let config = Configuration {
                client_id: client_id.into(),
                ..config()
            };
            let mut calls = 0;
            execute_with(&config, Operation::Scope, input(), |r| {
                calls += 1;
                if calls == 1 {
                    assert!(r.safe);
                    let mut installed = installation(json!({"contents":"write","workflows":"write","codespaces_secrets":"write","issues":"admin"}));
                    installed["client_id"] = json!(client_id);
                    return Ok(json!({"installations":[installed]}));
                }
                assert!(!r.safe);
                assert_eq!(
                    r.url,
                    format!("https://api.github.com/applications/{client_id}/token/scoped")
                );
                match r.authentication {
                    Authentication::App {
                        client_id: sent_client_id,
                        client_secret,
                    } => {
                        assert_eq!(sent_client_id, client_id);
                        assert_eq!(client_secret, "fixture-secret");
                    }
                    _ => panic!("App authentication required"),
                }
                assert_eq!(r.body["target_id"], 7);
                assert_eq!(r.body["repository_ids"], json!([11, 12]));
                assert_eq!(r.body["permissions"], json!({"contents":"read","issues":"read","metadata":"read"}));
                Ok(scoped())
            })
            .unwrap();
            assert_eq!(calls, 2);
        }
    }
    #[test]
    fn unsafe_client_ids_are_rejected_before_network() {
        for client_id in [
            "",
            "../token",
            ".",
            "..",
            "Iv1/test",
            "Iv1\\test",
            "Iv1.test?x=1",
            "Iv1.test#x",
            "Iv1.test%2Ftoken",
            "Iv1.test\n",
            "Iv1.test\0",
        ] {
            let config = Configuration {
                client_id: client_id.into(),
                ..config()
            };
            assert!(execute_with(
                &config,
                Operation::Refresh,
                json!({"refreshToken":"refresh"}),
                |_| panic!("unexpected network"),
            )
            .is_err());
        }
    }
    #[test]
    fn all_mode_omits_repo_filter_and_write_keeps_allowed_app_permissions() {
        let mut request = input();
        request["allRepositories"] = json!(true);
        request["repositoryIds"] = json!([]);
        request["allowChanges"] = json!(true);
        execute_with(&config(),Operation::Scope,request,|r| {if r.method==Method::GET {return Ok(json!({"installations":[installation(json!({"contents":"write","workflows":"write"}))]}));}assert!(r.body.get("repository_ids").is_none());assert_eq!(r.body["permissions"],json!({"contents":"write","workflows":"write","metadata":"read"}));Ok(scoped())}).unwrap();
    }
    /// Every permission an App can hold, at its highest level.
    fn broad_installation() -> Value {
        installation(json!({
            "actions":"write","administration":"write","checks":"write","codespaces_secrets":"write",
            "contents":"write","dependabot_secrets":"write","deployments":"write","environments":"write",
            "issues":"admin","metadata":"read","packages":"write","pages":"write","pull_requests":"write",
            "repository_hooks":"write","secret_scanning_alerts":"write","secrets":"write",
            "security_events":"write","statuses":"write","vulnerability_alerts":"write","workflows":"write",
        }))
    }
    fn scoped_permissions(request: Value, installation: Value) -> Result<Value, String> {
        let mut permissions = Value::Null;
        execute_with(&config(), Operation::Scope, request, |r| {
            if r.method == Method::GET {
                return Ok(json!({"installations":[installation.clone()]}));
            }
            permissions = r.body.clone();
            Ok(scoped())
        })?;
        Ok(permissions)
    }
    const NEVER_GRANTED: &[&str] = &[
        "administration",
        "secrets",
        "dependabot_secrets",
        "codespaces_secrets",
        "environments",
        "repository_hooks",
        "secret_scanning_alerts",
        "security_events",
    ];
    #[test]
    fn computer_tokens_use_explicit_allowlists_and_never_admin_or_secret_scopes() {
        let read = scoped_permissions(input(), broad_installation()).unwrap();
        assert_eq!(
            read["permissions"],
            json!({"metadata":"read","contents":"read","issues":"read","pull_requests":"read","statuses":"read","checks":"read"})
        );
        let mut request = input();
        request["allowChanges"] = json!(true);
        let write = scoped_permissions(request, broad_installation()).unwrap();
        // Allowing changes also lets agents edit workflows and read CI runs (owner answer).
        assert_eq!(
            write["permissions"],
            json!({"metadata":"read","contents":"write","issues":"write","pull_requests":"write","statuses":"write","checks":"read","workflows":"write","actions":"read"})
        );
        for body in [&read, &write] {
            for name in NEVER_GRANTED {
                assert!(body["permissions"].get(*name).is_none(), "{name} granted");
            }
        }
    }
    #[test]
    fn allowlists_never_exceed_what_the_installation_grants() {
        let mut request = input();
        request["allowChanges"] = json!(true);
        let body = scoped_permissions(
            request,
            installation(json!({"contents":"read","issues":"write","actions":"write"})),
        )
        .unwrap();
        assert_eq!(
            body["permissions"],
            json!({"metadata":"read","contents":"read","issues":"write","actions":"read"})
        );
    }
    #[test]
    fn host_push_tokens_write_contents_of_one_repository_only() {
        let mut request = input();
        request["purpose"] = json!("hostPush");
        request["allowChanges"] = json!(true);
        request["repositoryIds"] = json!([11]);
        let body = scoped_permissions(request.clone(), broad_installation()).unwrap();
        assert_eq!(
            body["permissions"],
            json!({"metadata":"read","contents":"write"})
        );
        assert_eq!(body["repository_ids"], json!([11]));
        // One bound repository: several repositories or all repositories never mint.
        for (key, value) in [
            ("repositoryIds", json!([11, 12])),
            ("purpose", json!("other")),
        ] {
            let mut invalid = request.clone();
            invalid[key] = value;
            assert!(
                execute_with(&config(), Operation::Scope, invalid, |_| panic!(
                    "unexpected request"
                ))
                .is_err()
            );
        }
        let mut all = request.clone();
        all["allRepositories"] = json!(true);
        all["repositoryIds"] = json!([]);
        assert!(execute_with(&config(), Operation::Scope, all, |_| panic!(
            "unexpected request"
        ))
        .is_err());
        let error =
            scoped_permissions(request, installation(json!({"contents":"read"}))).unwrap_err();
        assert!(error.contains("Contents"), "{error}");
    }
    #[test]
    fn invalid_scope_inputs_never_reach_github() {
        for (key, value) in [
            ("repositoryIds", json!([])),
            ("repositoryIds", json!([1, 1])),
            ("repositoryIds", json!([0])),
            ("repositoryIds", json!([9007199254740992u64])),
            ("repositoryIds", json!(vec![1; 501])),
            ("ownerId", json!(-1)),
            ("allowChanges", json!("yes")),
            ("allRepositories", json!(true)),
        ] {
            let mut request = input();
            request[key] = value;
            assert!(
                execute_with(&config(), Operation::Scope, request, |_| panic!(
                    "unexpected request"
                ))
                .is_err()
            );
        }
    }
    #[test]
    fn incompatible_installations_and_permissions_never_mint() {
        let valid = installation(json!({"contents":"read"}));
        let mut other_app = valid.clone();
        other_app["client_id"] = json!("other");
        let mut other_owner = valid.clone();
        other_owner["account"]["id"] = json!(8);
        let mut suspended = valid.clone();
        suspended["suspended_at"] = json!("2026-01-01");
        for item in [
            other_app,
            other_owner,
            suspended,
            installation(json!({"organization_administration":"write"})),
            installation(json!({"unknown":"read"})),
            installation(json!({"contents":"owner"})),
        ] {
            assert!(execute_with(&config(), Operation::Scope, input(), |r| {
                assert_eq!(r.method, Method::GET);
                Ok(json!({"installations":[item.clone()]}))
            })
            .is_err());
        }
    }
    #[test]
    fn installations_without_optional_client_id_can_mint() {
        let mut installed = installation(json!({"contents":"read"}));
        installed.as_object_mut().unwrap().remove("client_id");
        let body = scoped_permissions(input(), installed).unwrap();
        assert_eq!(body["target_id"], 7);
        assert_eq!(body["repository_ids"], json!([11, 12]));
        assert_eq!(
            body["permissions"],
            json!({"contents":"read","metadata":"read"})
        );
    }
    #[test]
    fn malformed_installation_client_ids_never_mint() {
        for client_id in [Value::Null, json!(7), json!({}), json!([])] {
            let mut installed = installation(json!({"contents":"read"}));
            installed["client_id"] = client_id;
            assert!(execute_with(&config(), Operation::Scope, input(), |r| {
                assert_eq!(r.method, Method::GET);
                Ok(json!({"installations":[installed.clone()]}))
            })
            .is_err());
        }
    }
    #[test]
    fn pagination_is_bounded_and_expired_tokens_fail_closed() {
        let mut calls = 0;
        assert!(execute_with(&config(), Operation::Scope, input(), |_| {
            calls += 1;
            Ok(json!({"installations":vec![json!({});100]}))
        })
        .is_err());
        assert_eq!(calls, 20);
        for result in [
            json!({"token":"scoped","expires_at":"yesterday"}),
            json!({"token":"scoped","expires_at":"2000-01-01T00:00:00Z"}),
            json!({"token":"","expires_at":"2099-01-01T00:00:00Z"}),
        ] {
            assert!(execute_with(&config(), Operation::Scope, input(), |r| {
                if r.method == Method::GET {
                    Ok(json!({"installations":[installation(json!({"contents":"read"}))]}))
                } else {
                    Ok(result.clone())
                }
            })
            .is_err());
        }
    }
}
