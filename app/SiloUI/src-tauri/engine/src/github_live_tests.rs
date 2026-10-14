//! Opt-in authenticated regression. All diagnostics deliberately omit HTTP bodies and credentials.
use crate::github_tokens::{execute, Configuration, Operation};
use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::{collections::HashMap, time::Duration};

type Check<T> = Result<T, String>;
struct Repository {
    name: String,
    id: u64,
}
struct Fixture {
    app: Configuration,
    parent: String,
    repos: [Repository; 3],
}
fn required(env: &HashMap<String, String>, name: &str) -> Check<String> {
    env.get(name)
        .filter(|v| !v.is_empty() && !v.bytes().any(|c| c <= 32 || c == 127))
        .cloned()
        .ok_or_else(|| "Missing or invalid regression configuration.".into())
}
fn authorize_fixtures(env: &HashMap<String, String>) -> Check<()> {
    crate::test_support::live::validate_confirmation(
        env.get(crate::test_support::live::CONFIRM_VARIABLE)
            .map(String::as_str),
    )
    .map_err(str::to_owned)?;
    ensure(
        env.get("SILO_GITHUB_TEST_CONFIRM").map(String::as_str)
            == Some("private-test-repositories"),
        "Explicit private fixture authorization is required.",
    )
}
fn configuration(env: &HashMap<String, String>) -> Check<Fixture> {
    authorize_fixtures(env)?;
    let repository = |role: &str| -> Check<Repository> {
        let name = required(env, &format!("SILO_GITHUB_TEST_{role}_REPO"))?;
        let parts: Vec<_> = name.split('/').collect();
        ensure(
            parts.len() == 2
                && !parts[0].is_empty()
                && parts[0]
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-')
                && !parts[1].is_empty()
                && ![".", ".."].contains(&parts[1])
                && parts[1]
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c)),
            "Invalid fixture repository name.",
        )?;
        let id = required(env, &format!("SILO_GITHUB_TEST_{role}_REPO_ID"))?
            .parse::<u64>()
            .map_err(|_| "Invalid fixture repository ID.")?;
        ensure(
            id > 0 && id <= 9_007_199_254_740_991,
            "Invalid fixture repository ID.",
        )?;
        Ok(Repository { name, id })
    };
    let repos = [
        repository("READ")?,
        repository("WRITE")?,
        repository("DENIED")?,
    ];
    for (i, a) in repos.iter().enumerate() {
        for b in repos.iter().skip(i + 1) {
            ensure(
                a.id != b.id
                    && !a.name.eq_ignore_ascii_case(&b.name)
                    && a.name
                        .split('/')
                        .next()
                        .unwrap()
                        .eq_ignore_ascii_case(b.name.split('/').next().unwrap()),
                "Use three distinct private fixture repositories under one owner.",
            )?;
        }
    }
    Ok(Fixture {
        app: Configuration {
            client_id: required(env, "SILO_GITHUB_CLIENT_ID")?,
            client_secret: required(env, "SILO_GITHUB_CLIENT_SECRET")?,
        },
        parent: required(env, "SILO_GITHUB_TEST_USER_TOKEN")?,
        repos,
    })
}
fn ensure(value: bool, message: &'static str) -> Check<()> {
    if value {
        Ok(())
    } else {
        Err(message.into())
    }
}
// Never format reqwest errors: they can contain credential-bearing URLs.
fn transport_failure(error: reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "GitHub request timed out; mutations were not retried."
    } else if error.is_connect() {
        "GitHub connection failed; mutations were not retried."
    } else if error.is_decode() {
        "GitHub response was invalid."
    } else {
        "GitHub request failed; mutations were not retried."
    }
}
struct Api {
    client: Client,
}
impl Api {
    fn call(&self, token: &str, path: &str, body: Option<Value>) -> Check<(u16, Value)> {
        let builder = if body.is_some() {
            self.client.post(format!("https://api.github.com{path}"))
        } else {
            self.client.get(format!("https://api.github.com{path}"))
        };
        let builder = builder
            .bearer_auth(token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        let builder = if let Some(body) = body {
            builder.json(&body)
        } else {
            builder
        };
        let response = builder.send().map_err(transport_failure)?;
        let status = response.status().as_u16();
        let data = response.json().map_err(transport_failure)?;
        Ok((status, data))
    }
    fn graph(&self, token: &str, query: &str, variables: Value) -> Check<(u16, Value)> {
        self.call(
            token,
            "/graphql",
            Some(json!({"query":query,"variables":variables})),
        )
    }
}
fn repository_denied(response: &(u16, Value)) -> bool {
    matches!(
        (response.0, response.1["message"].as_str()),
        (404, Some("Not Found"))
            | (403, Some("Resource not accessible by integration"))
            | (
                403,
                Some("Resource not accessible by personal access token")
            )
    )
}
fn escalation_denied(operation: &str, response: &(u16, Value)) -> bool {
    repository_denied(response)
        || (operation == "token/scoped"
            && response.0 == 401
            && response.1["message"].as_str()
                == Some("A scoped token cannot create another scoped token."))
}
// Categories aid diagnosis without emitting untrusted response text.
// They never determine whether a permission assertion passes.
fn failure_category(data: &Value) -> &'static str {
    match data["message"].as_str() {
        Some("Bad credentials") => "bad credentials",
        Some("Validation Failed") => "validation failure",
        Some("Not Found") => "not found",
        Some(message) => {
            let lower = message.to_ascii_lowercase();
            if lower.contains("rate limit") {
                "rate limit"
            } else if lower.contains("scop") && lower.contains("token") {
                "token scope restriction"
            } else if lower.contains("token") {
                "other token error"
            } else {
                "unrecognized error"
            }
        }
        None => "missing error message",
    }
}
fn safe_failure_message(data: &Value) -> String {
    let message = data["message"].as_str().unwrap_or("");
    let mut safe = String::new();
    for chunk in message.split_whitespace().take(200) {
        let word = if (1..=14).contains(&chunk.len())
            && chunk.bytes().all(|byte| byte.is_ascii_alphabetic())
        {
            chunk
        } else {
            "[redacted]"
        };
        if !safe.is_empty() {
            safe.push(' ');
        }
        safe.push_str(word);
        if safe.len() >= 200 {
            safe.truncate(200);
            break;
        }
    }
    safe
}
fn graph_ok(response: &(u16, Value)) -> bool {
    response.0 == 200 && response.1.get("errors").is_none()
}
fn graph_denied(response: &(u16, Value)) -> bool {
    response.0 == 200
        && response.1["errors"].as_array().is_some_and(|errors| {
            !errors.is_empty()
                && errors
                    .iter()
                    .all(|e| matches!(e["type"].as_str(), Some("FORBIDDEN" | "NOT_FOUND")))
        })
}
fn scope(
    app: &Configuration,
    parent: &str,
    owner: u64,
    ids: &[u64],
    write: bool,
    children: &mut Vec<String>,
) -> Check<(String, u64)> {
    let value = execute(
        app,
        Operation::Scope,
        json!({"accessToken":parent,"ownerId":owner,"repositoryIds":ids,"allRepositories":ids.is_empty(),"allowChanges":write}),
    )?;
    let token = value["accessToken"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("Invalid scoped token.")?
        .to_owned();
    children.push(token.clone());
    let expiry = time::OffsetDateTime::parse(
        value["expiresAt"].as_str().ok_or("Missing token expiry.")?,
        &time::format_description::well_known::Rfc3339,
    )
    .map_err(|_| "Invalid token expiry.")?
    .unix_timestamp();
    ensure(
        expiry > time::OffsetDateTime::now_utc().unix_timestamp() + 120,
        "Scoped token expires too soon.",
    )?;
    Ok((token, expiry as u64))
}
const QUERY: &str =
    "query($owner:String!,$name:String!){repository(owner:$owner,name:$name){id nameWithOwner}}";
const UPDATE: &str =
    "mutation($input:UpdateIssueInput!){updateIssue(input:$input){issue{id title}}}";
fn verify(f: Fixture, computer: bool) -> Check<()> {
    let api = Api {
        client: Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("Silo-authenticated-regression")
            .build()
            .map_err(|_| "Cannot initialize regression client.")?,
    };
    let mut children = Vec::new();
    let mut issue_id = None;
    let mut stage = "fixture preflight";
    let result = (|| -> Check<()> {
        let mut repositories = Vec::new();
        for repo in &f.repos {
            let response = api.call(&f.parent, &format!("/repos/{}", repo.name), None)?;
            ensure(
                response.0 == 200
                    && response.1["id"].as_u64() == Some(repo.id)
                    && response.1["private"] == true
                    && response.1["full_name"]
                        .as_str()
                        .is_some_and(|name| name.eq_ignore_ascii_case(&repo.name)),
                "Fixture privacy, exact name, ID or parent authorization preflight failed.",
            )?;
            repositories.push(response.1);
        }
        let owner = repositories[0]["owner"]["id"]
            .as_u64()
            .ok_or("Missing fixture owner ID.")?;
        ensure(
            repositories
                .iter()
                .all(|r| r["owner"]["id"].as_u64() == Some(owner))
                && repositories[1]["has_issues"] == true,
            "Fixture owner or Issues preflight failed.",
        )?;
        stage = "create read token";
        let (read, read_expiry) = scope(
            &f.app,
            &f.parent,
            owner,
            &[f.repos[0].id, f.repos[1].id],
            false,
            &mut children,
        )?;
        stage = "create write token";
        let (write, write_expiry) = scope(
            &f.app,
            &f.parent,
            owner,
            &[f.repos[1].id],
            true,
            &mut children,
        )?;
        stage = "REST and GraphQL repository boundaries";
        for (grant, token, allowed) in [
            ("read", &read, [true, true, false]),
            ("write", &write, [false, true, false]),
        ] {
            for ((fixture, repo), allowed) in ["read", "write", "denied"]
                .into_iter()
                .zip(&f.repos)
                .zip(allowed)
            {
                let response = api.call(token, &format!("/repos/{}", repo.name), None)?;
                let actual = response.0;
                let expected = if allowed { "200" } else { "403 authorization denial or 404 Not Found" };
                if !(if allowed { actual == 200 } else { repository_denied(&response) }) {
                    return Err(format!(
                        "REST repository boundary failed: {grant} grant, {fixture} fixture, HTTP {actual}, expected {expected}."
                    ));
                }
                let parts: Vec<_> = repo.name.split('/').collect();
                let response =
                    api.graph(token, QUERY, json!({"owner":parts[0],"name":parts[1]}))?;
                ensure(
                    if allowed {
                        graph_ok(&response)
                            && response.1["data"]["repository"]["nameWithOwner"]
                                .as_str()
                                .is_some_and(|n| n.eq_ignore_ascii_case(&repo.name))
                    } else {
                        graph_denied(&response) && response.1["data"]["repository"].is_null()
                    },
                    "GraphQL repository boundary failed.",
                ).map_err(|_| format!(
                    "GraphQL repository boundary failed: {grant} grant, {fixture} fixture, HTTP {}, expected access {}.",
                    response.0, if allowed { "allowed" } else { "denied" }
                ))?;
            }
        }
        stage = "denied GraphQL node read";
        let node = api.graph(
            &read,
            "query($id:ID!){node(id:$id){... on Repository{id nameWithOwner}}}",
            json!({"id":repositories[2]["node_id"]}),
        )?;
        ensure(
            graph_denied(&node) && node.1["data"]["node"].is_null(),
            "GraphQL node ID exposed denied repository.",
        )?;
        stage = "create fixture issue";
        let marker = format!("Silo authenticated regression {}", uuid::Uuid::new_v4());
        let created = api.graph(&write, "mutation($input:CreateIssueInput!){createIssue(input:$input){issue{id number title}}}", json!({"input":{"repositoryId":repositories[1]["node_id"],"title":marker,"body":"Permanent Silo integration test; closed during cleanup."}}))?;
        issue_id = created.1["data"]["createIssue"]["issue"]["id"]
            .as_str()
            .map(str::to_owned);
        ensure(
            graph_ok(&created) && issue_id.is_some(),
            "Could not create isolated fixture issue.",
        )?;
        let number = created.1["data"]["createIssue"]["issue"]["number"]
            .as_u64()
            .ok_or("Missing fixture issue number.")?;
        stage = "denied GraphQL write";
        let denied = api.graph(
            &read,
            UPDATE,
            json!({"input":{"id":issue_id,"title":"unexpected read mutation"}}),
        )?;
        ensure(
            graph_denied(&denied) && denied.1["data"]["updateIssue"]["issue"].is_null(),
            "Read token performed a node-ID write.",
        )?;
        stage = "denied REST write";
        let rest_denied = api
            .client
            .patch(format!(
                "https://api.github.com/repos/{}/issues/{number}",
                f.repos[1].name
            ))
            .bearer_auth(&read)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .json(&json!({"title":"unexpected REST read mutation"}))
            .send()
            .map_err(transport_failure)?;
        let status = rest_denied.status().as_u16();
        let data = rest_denied.json().map_err(transport_failure)?;
        ensure(
            repository_denied(&(status, data)),
            "Read token performed REST write or returned inconclusive response.",
        )?;
        stage = "verify denied writes left issue unchanged";
        let unchanged = api.call(
            &f.parent,
            &format!("/repos/{}/issues/{number}", f.repos[1].name),
            None,
        )?;
        ensure(
            unchanged.0 == 200 && unchanged.1["title"] == marker,
            "Denied mutation changed issue.",
        )?;
        stage = "allowed GraphQL write";
        let updated = api.graph(
            &write,
            UPDATE,
            json!({"input":{"id":issue_id,"title":format!("{marker} verified")}}),
        )?;
        ensure(graph_ok(&updated), "Allowed node-ID write failed.")?;
        // Ask GitHub to widen a child, as an attacker knowing the App client secret could.
        // Either refusal or an equally restricted result is safe. Track any issued token.
        stage = "child token escalation through native scope";
        let attempted = execute(
            &f.app,
            Operation::Scope,
            json!({"accessToken":read,"ownerId":owner,"repositoryIds":[],"allRepositories":true,"allowChanges":true}),
        );
        // A native error is not authorization evidence: the mandatory direct API
        // probes below independently verify that GitHub rejects escalation.
        if let Ok(value) = attempted {
            let token = value["accessToken"]
                .as_str()
                .ok_or("Invalid re-scoped child.")?
                .to_owned();
            children.push(token.clone());
            ensure(
                repository_denied(&api.call(&token, &format!("/repos/{}", f.repos[2].name), None)?),
                "Child re-scope expanded repository access.",
            )?;
            ensure(
                graph_denied(&api.graph(
                    &token,
                    UPDATE,
                    json!({"input":{"id":issue_id,"title":"unexpected child escalation"}}),
                )?),
                "Child re-scope expanded write access.",
            )?;
        }
        stage = "create escalation probe token";
        let (probe_read, _) = scope(
            &f.app,
            &f.parent,
            owner,
            &[f.repos[0].id, f.repos[1].id],
            false,
            &mut children,
        )?;
        // Bypass Silo's scope preparation: GitHub itself must enforce parent bounds.
        stage = "direct child scope and reset escalation";
        for (method, suffix, body) in [
            (
                reqwest::Method::POST,
                "token/scoped",
                json!({"access_token":probe_read,"target_id":owner,"repository_ids":[f.repos[2].id],"permissions":{"contents":"write","issues":"write","metadata":"read"}}),
            ),
            (
                reqwest::Method::PATCH,
                "token",
                json!({"access_token":probe_read}),
            ),
        ] {
            let response = api
                .client
                .request(
                    method,
                    format!(
                        "https://api.github.com/applications/{}/{suffix}",
                        f.app.client_id
                    ),
                )
                .basic_auth(&f.app.client_id, Some(&f.app.client_secret))
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .json(&body)
                .send()
                .map_err(transport_failure)?;
            let status = response.status().as_u16();
            let data: Value = response
                .json()
                .map_err(transport_failure)?;
            if status == 201 || status == 200 {
                let token = data["token"]
                    .as_str()
                    .or_else(|| data["access_token"].as_str())
                    .ok_or("Child escalation response omitted token.")?
                    .to_owned();
                children.push(token.clone());
                ensure(
                    repository_denied(&api.call(&token, &format!("/repos/{}", f.repos[2].name), None)?),
                    "GitHub child re-scope or reset expanded repository access.",
                )?;
                ensure(graph_denied(&api.graph(&token, UPDATE, json!({"input":{"id":issue_id,"title":"unexpected direct child escalation"}}))?), "GitHub child re-scope or reset expanded write access.")?;
            } else {
                let category = failure_category(&data);
                let message = safe_failure_message(&data);
                if !escalation_denied(suffix, &(status, data)) {
                    return Err(format!(
                        "Escalation probe {suffix}: HTTP {status}, {category}, {message}; permission refusal unconfirmed."
                    ));
                }
            }
        }
        let unchanged = api.call(
            &f.parent,
            &format!("/repos/{}/issues/{number}", f.repos[1].name),
            None,
        )?;
        ensure(
            unchanged.0 == 200 && unchanged.1["title"] == format!("{marker} verified"),
            "Child escalation changed the fixture issue.",
        )?;
        stage = "create all-repositories token";
        let (all, _) = scope(&f.app, &f.parent, owner, &[], false, &mut children)?;
        stage = "all-repositories boundaries";
        for repo in &f.repos {
            ensure(
                api.call(&all, &format!("/repos/{}", repo.name), None)?.0 == 200,
                "All repositories excluded authorized fixture.",
            )?;
        }
        stage = "child revocation";
        execute(&f.app, Operation::RevokeToken, json!({"accessToken":all}))
            .map_err(|_| "Child revocation failed.")?;
        ensure(
            api.call(&all, "/user", None)?.0 == 401
                && api.call(&f.parent, "/user", None)?.0 == 200
                && api
                    .call(&read, &format!("/repos/{}", f.repos[0].name), None)?
                    .0
                    == 200,
            "Child revocation affected parent or sibling.",
        )?;
        children.retain(|token| token != &all);
        stage = "authenticated computer workflow";
        if computer {
            run_computer(
                &f,
                issue_id.as_deref().ok_or("Missing fixture issue ID.")?,
                json!({"version":1,"owners":[{"login":f.repos[0].name.split('/').next().unwrap(),"repositoryIds":[f.repos[0].id,f.repos[1].id],"readToken":read,"writeToken":write,"expiresAt":read_expiry.min(write_expiry)}]}),
            )?;
        }
        Ok(())
    })().map_err(|error| format!("{stage}: {error}"));
    let mut cleanup_failed = false;
    if let Some(id) = issue_id {
        cleanup_failed |= !api
            .graph(
                &f.parent,
                "mutation($input:CloseIssueInput!){closeIssue(input:$input){issue{state}}}",
                json!({"input":{"issueId":id}}),
            )
            .is_ok_and(|r| graph_ok(&r) && r.1["data"]["closeIssue"]["issue"]["state"] == "CLOSED");
    }
    for token in children {
        cleanup_failed |=
            execute(&f.app, Operation::RevokeToken, json!({"accessToken":token})).is_err();
    }
    ensure(!cleanup_failed, "Cleanup unconfirmed: inspect only named fixture repository for test issue and revoke remaining test credentials.")?;
    result
}
fn guest_regression_command(executable: &std::path::Path) -> std::process::Command {
    // Reuse the already-built test harness: nested Cargo would share its build
    // locks and could compile another harness against a changing target directory.
    let mut command = std::process::Command::new(executable);
    command.args([
        "runtime::github_integration_tests::github_authenticated_guest_workflow",
        "--exact",
        "--ignored",
        "--test-threads=1",
    ]);
    command
}
fn run_computer(f: &Fixture, issue_id: &str, profile: Value) -> Check<()> {
    let executable = std::env::current_exe()
        .map_err(|_| "Cannot locate authenticated guest regression executable.")?;
    let mut command = guest_regression_command(&executable);
    command
        .current_dir(crate::test_support::manifest_root())
        .env_clear();
    for key in [
        "PATH",
        "HOME",
        "TMPDIR",
        "SDKROOT",
        "DEVELOPER_DIR",
        "SILO_TEST_MSB",
        "SILO_TEST_LIBKRUNFW",
        "SILO_TEST_GIT",
        "SILO_TEST_GIT_SUPPORT",
        "SILO_GITHUB_TEST_INFLIGHT",
        crate::test_support::live::CONFIRM_VARIABLE,
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    for (role, repo) in ["READ", "WRITE", "DENIED"].into_iter().zip(&f.repos) {
        command.env(format!("SILO_GITHUB_TEST_{role}_REPO"), &repo.name);
    }
    let output = command
        .env("SILO_TEST_GITHUB_PROFILE_JSON", profile.to_string())
        .env("SILO_TEST_GITHUB_ISSUE_ID", issue_id)
        .output()
        .map_err(|_| "Cannot launch authenticated guest regression.")?;
    // Emit only fixed probe diagnoses, never arbitrary child output or credentials.
    for diagnosis in [
        "Live socket probe inconclusive: authenticated connection did not stay open.",
        "Live socket probe failed: policy change did not close held connection within five seconds.",
    ] {
        if !output.status.success() && String::from_utf8_lossy(&output.stdout).contains(diagnosis) {
            return Err(diagnosis.into());
        }
    }
    ensure(
        output.status.success(),
        "Authenticated guest regression failed; output suppressed to protect credentials.",
    )
}
/// Separate browser authorization: never reads or replaces Silo's saved account.
#[test]
#[ignore = "requires an isolated PKCE callback and explicitly authorized private fixture repositories"]
fn github_authenticated_browser_workflow() {
    let mut environment: HashMap<String, String> = std::env::vars().collect();
    authorize_fixtures(&environment)
        .expect("Explicit fixture authorization required before exchange");
    let app = Configuration {
        client_id: required(&environment, "SILO_GITHUB_CLIENT_ID").unwrap(),
        client_secret: required(&environment, "SILO_GITHUB_CLIENT_SECRET").unwrap(),
    };
    let session = execute(
        &app,
        Operation::Exchange,
        json!({
            "code": required(&environment, "SILO_GITHUB_TEST_CODE").unwrap(),
            "codeVerifier": required(&environment, "SILO_GITHUB_TEST_VERIFIER").unwrap(),
            "redirectUri": required(&environment, "SILO_GITHUB_TEST_REDIRECT").unwrap(),
        }),
    )
    .expect("Isolated native PKCE exchange failed.");
    let first = session["accessToken"]
        .as_str()
        .expect("Missing isolated token.")
        .to_owned();
    let mut current = first.clone();
    let result = (|| -> Check<()> {
        let renewed = execute(
            &app,
            Operation::Refresh,
            json!({
                "refreshToken":session["refreshToken"],
            }),
        )
        .map_err(|_| "Isolated native refresh failed.")?;
        current = renewed["accessToken"]
            .as_str()
            .ok_or("Missing renewed token.")?
            .to_owned();
        ensure(
            current != first && renewed["refreshToken"] != session["refreshToken"],
            "GitHub did not rotate isolated access and refresh credentials.",
        )?;
        environment.insert("SILO_GITHUB_TEST_USER_TOKEN".into(), current.clone());
        verify(configuration(&environment)?, true)
    })();
    let cleanup = execute(&app, Operation::RevokeToken, json!({"accessToken":current}));
    assert!(cleanup.is_ok(), "Isolated browser token cleanup failed.");
    assert!(result.is_ok(), "{}", result.unwrap_err());
}

#[test]
#[ignore = "requires explicit private GitHub fixtures and credentials; mutates only a marked fixture issue"]
fn github_authenticated_native_workflow() {
    let env = std::env::vars().collect();
    let result = configuration(&env).and_then(|f| {
        verify(
            f,
            std::env::var("SILO_GITHUB_TEST_VM").as_deref() == Ok("1"),
        )
    });
    assert!(
        result.is_ok(),
        "{}",
        result
            .err()
            .unwrap_or_else(|| "Authenticated regression failed.".into())
    );
}

#[test]
fn live_configuration_rejects_missing_confirmation_and_unsafe_fixtures() {
    let mut env: HashMap<String, String> = [
        (
            crate::test_support::live::CONFIRM_VARIABLE,
            crate::test_support::live::CONFIRM_VALUE,
        ),
        ("SILO_GITHUB_TEST_CONFIRM", "private-test-repositories"),
        ("SILO_GITHUB_CLIENT_ID", "app"),
        ("SILO_GITHUB_CLIENT_SECRET", "secret"),
        ("SILO_GITHUB_TEST_USER_TOKEN", "parent"),
        ("SILO_GITHUB_TEST_READ_REPO", "owner/read"),
        ("SILO_GITHUB_TEST_READ_REPO_ID", "1"),
        ("SILO_GITHUB_TEST_WRITE_REPO", "owner/write"),
        ("SILO_GITHUB_TEST_WRITE_REPO_ID", "2"),
        ("SILO_GITHUB_TEST_DENIED_REPO", "owner/denied"),
        ("SILO_GITHUB_TEST_DENIED_REPO_ID", "3"),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v.into()))
    .collect();
    assert!(configuration(&env).is_ok());
    for (key, value) in [
        (crate::test_support::live::CONFIRM_VARIABLE, ""),
        ("SILO_GITHUB_TEST_CONFIRM", ""),
        ("SILO_GITHUB_TEST_READ_REPO", "owner/.."),
        ("SILO_GITHUB_TEST_WRITE_REPO", "other/write"),
        ("SILO_GITHUB_TEST_WRITE_REPO", "OWNER/READ"),
        ("SILO_GITHUB_TEST_WRITE_REPO_ID", "1"),
        ("SILO_GITHUB_TEST_READ_REPO_ID", "0"),
        ("SILO_GITHUB_TEST_READ_REPO_ID", "9007199254740992"),
        ("SILO_GITHUB_TEST_USER_TOKEN", "secret\n"),
    ] {
        let original = env.insert(key.into(), value.into()).unwrap();
        assert!(
            configuration(&env).is_err(),
            "Unsafe fixture accepted for {key}"
        );
        env.insert(key.into(), original);
    }
}

#[test]
fn live_denial_requires_github_permission_evidence() {
    assert!(graph_denied(&(
        200,
        json!({"errors":[{"type":"FORBIDDEN"}]})
    )));
    assert!(graph_denied(&(
        200,
        json!({"errors":[{"type":"NOT_FOUND"}]})
    )));
    for response in [
        (429, json!({"errors":[{"type":"FORBIDDEN"}]})),
        (500, json!({"errors":[{"type":"FORBIDDEN"}]})),
        (200, json!({"errors":[{"type":"RATE_LIMITED"}]})),
        (
            200,
            json!({"errors":[{"type":"FORBIDDEN"},{"type":"RATE_LIMITED"}]}),
        ),
        (200, json!({"errors":[]})),
        (200, json!({"data":{"repository":null}})),
    ] {
        assert!(!graph_denied(&response));
    }
}

#[test]
fn live_transport_diagnostics_do_not_format_request_errors() {
    let error = Client::new()
        .get("http://user:fixture-secret@[invalid")
        .build()
        .expect_err("invalid destination must fail before sending");
    assert_eq!(
        transport_failure(error),
        "GitHub request failed; mutations were not retried."
    );
}

#[test]
fn live_rest_denial_requires_github_permission_evidence() {
    for (status, message) in [
        (404, "Not Found"),
        (403, "Resource not accessible by integration"),
        (403, "Resource not accessible by personal access token"),
    ] {
        assert!(repository_denied(&(status, json!({"message": message}))));
    }
    for (status, message) in [
        (403, "API rate limit exceeded"),
        (403, "You have exceeded a secondary rate limit"),
        (403, "Forbidden"),
        (403, "Not Found"),
        (404, "Unknown failure"),
        (429, "Resource not accessible by integration"),
        (500, "Resource not accessible by integration"),
        (200, "Resource not accessible by integration"),
    ] {
        assert!(!repository_denied(&(status, json!({"message": message}))));
    }
    assert!(!repository_denied(&(403, json!({}))));
    assert!(!repository_denied(&(404, json!({}))));
}

#[test]
fn live_error_categories_never_include_response_text() {
    assert_eq!(
        failure_category(&json!({"message":"fixture-secret token cannot be scoped"})),
        "token scope restriction"
    );
    assert_eq!(
        failure_category(&json!({"message":"fixture-secret rate limit"})),
        "rate limit"
    );
    assert_eq!(
        failure_category(&json!({"message":"fixture-secret"})),
        "unrecognized error"
    );
    assert_eq!(
        failure_category(&json!({"token":"fixture-secret"})),
        "missing error message"
    );
}

#[test]
fn live_failure_message_redacts_tokens_urls_and_long_values() {
    let value = json!({"message":"This token cannot be scoped ghu_0123456789abcdef0123456789abcdef https://user:secret@example.com/secret ABCDEFGHIJKLMNOP 123456 secret=value"});
    assert_eq!(
        safe_failure_message(&value),
        "This token cannot be scoped [redacted] [redacted] [redacted] [redacted] [redacted]"
    );
    assert_eq!(
        safe_failure_message(&json!({"message":"word ".repeat(1000)})).len(),
        200
    );
    assert_eq!(safe_failure_message(&json!({"token":"secret"})), "");
}

#[test]
fn live_scoped_parent_refusal_requires_exact_operation_status_and_message() {
    let refusal = json!({"message":"A scoped token cannot create another scoped token."});
    assert!(escalation_denied("token/scoped", &(401, refusal.clone())));
    assert!(!escalation_denied("token", &(401, refusal.clone())));
    assert!(!escalation_denied("token/scoped", &(403, refusal)));
    assert!(!escalation_denied(
        "token/scoped",
        &(401, json!({"message":"Bad credentials"}))
    ));
    assert!(!escalation_denied(
        "token/scoped",
        &(
            401,
            json!({"message":"A scoped token cannot create another scoped credential."})
        )
    ));
}

#[test]
fn guest_regression_uses_the_current_test_harness_without_cargo() {
    let executable = std::env::current_exe().unwrap();
    let command = guest_regression_command(&executable);
    assert_eq!(command.get_program(), executable.as_os_str());
    let args: Vec<_> = command
        .get_args()
        .map(|arg| arg.to_str().unwrap())
        .collect();
    assert_eq!(
        args,
        [
            "runtime::github_integration_tests::github_authenticated_guest_workflow",
            "--exact",
            "--ignored",
            "--test-threads=1",
        ]
    );
    // The exact selector must resolve to a registered test rather than silently
    // succeeding with zero executed tests. Listing cannot run live regressions.
    let output = std::process::Command::new(&executable)
        .arg("--list")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .any(|line| line == format!("{}: test", args[0])));
}
