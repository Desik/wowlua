//! The registry's protocol (spec §10.10.1–10.10.2): `config.json`, the
//! sparse index with ETags, downloads, publish, yank and unyank, owners,
//! and the admins' reserved names. Errors are
//! `{"errors": [{"code", "detail"}]}` with 400, 401, 403, 404, 409 or 413.

use crate::store::{PublishError, Reserved, same_login};
use crate::{Shared, auth};
use axum::extract::multipart::MultipartRejection;
use axum::extract::{Multipart, Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use std::collections::BTreeMap;
use wowlua_pkg::registry::{self, IndexDep, IndexEntry};

/// An error answer.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub detail: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, detail: impl Into<String>) -> Self {
        ApiError {
            status,
            code,
            detail: detail.into(),
        }
    }

    fn bad(code: &'static str, detail: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, detail)
    }

    fn internal(detail: impl std::fmt::Display) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            format!("the registry could not store the change: {detail}"),
        )
    }

    fn too_large(max: usize) -> Self {
        Self::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "too-large",
            format!(
                "the archive is larger than the registry's limit of {} KiB",
                max / 1024
            ),
        )
    }

    fn not_found(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not-found", detail)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({
            "errors": [{ "code": self.code, "detail": self.detail }]
        });
        (
            self.status,
            [(header::CONTENT_TYPE, "application/json")],
            body.to_string(),
        )
            .into_response()
    }
}

type Result<T> = std::result::Result<T, ApiError>;

fn json(status: StatusCode, value: serde_json::Value) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        value.to_string(),
    )
        .into_response()
}

/// Runs blocking work (files, hashing, GitHub) off the async threads.
pub async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    tokio::task::spawn_blocking(f)
        .await
        .expect("a blocking task of the registry panicked")
}

/// The user an `Authorization: Bearer <token>` names (401 otherwise).
pub fn user(app: &Shared, headers: &HeaderMap) -> Result<String> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.strip_prefix("Bearer ")
                .or_else(|| v.strip_prefix("bearer "))
        })
        .map(str::trim)
        .filter(|t| !t.is_empty());
    let Some(token) = token else {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            format!(
                "this request needs an API token (`Authorization: Bearer <token>`); sign in at {}/login to create one",
                app.config.public_url
            ),
        ));
    };
    app.store
        .token_login(&auth::hash_token(token))
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "the API token is unknown or revoked",
            )
        })
}

fn check_name(name: &str) -> Result<()> {
    if registry::valid_name(name) {
        Ok(())
    } else {
        Err(ApiError::bad(
            "invalid-name",
            format!("`{name}` is not a package name: use 2–64 lowercase letters, digits and `-`"),
        ))
    }
}

/// The user may change the package: an owner, or an admin.
fn check_owner(app: &Shared, name: &str, login: &str) -> Result<()> {
    let owners = app.store.owners(name);
    if app.store.entries(name).is_empty() {
        return Err(ApiError::not_found(format!("no package `{name}`")));
    }
    if owners.iter().any(|o| same_login(o, login)) || app.config.is_admin(login) {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "not-owner",
            format!("`{login}` is not an owner of `{name}`"),
        ))
    }
}

pub async fn not_found() -> ApiError {
    ApiError::not_found("no such page")
}

// ---- index and downloads ---------------------------------------------------

pub async fn config(State(app): State<Shared>) -> Response {
    let url = &app.config.public_url;
    json(
        StatusCode::OK,
        serde_json::json!({ "api": url, "dl": format!("{url}/dl") }),
    )
}

/// The index file, with a strong ETag of its contents.
pub async fn index(
    State(app): State<Shared>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !registry::valid_name(&name) {
        return ApiError::not_found(format!("no package `{name}`")).into_response();
    }
    let text = blocking({
        let app = app.clone();
        move || app.store.index_text(&name).map(|t| (name, t))
    })
    .await;
    let Some((name, text)) = text else {
        return ApiError::not_found("no such package").into_response();
    };
    let _ = name;
    let etag = format!("\"{}\"", &auth::hash_token(&text)[..32]);
    let matches = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(',')
                .map(|t| t.trim().trim_start_matches("W/"))
                .any(|t| t == etag || t == "*")
        });
    let mut response = if matches {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/x-ndjson")],
            text,
        )
            .into_response()
    };
    let h = response.headers_mut();
    if let Ok(v) = HeaderValue::from_str(&etag) {
        h.insert(header::ETAG, v);
    }
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
}

/// `GET {dl}/<name>/<version>.tar.gz`; yanked versions stay downloadable.
pub async fn download(
    State(app): State<Shared>,
    Path((name, file)): Path<(String, String)>,
) -> Response {
    let Some(version) = file.strip_suffix(".tar.gz") else {
        return ApiError::not_found("no such file").into_response();
    };
    if !registry::valid_name(&name) || semver::Version::parse(version).is_err() {
        return ApiError::not_found("no such file").into_response();
    }
    let path = app.store.archive_path(&name, version);
    match blocking(move || std::fs::read(path)).await {
        Ok(bytes) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "application/gzip"),
                (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => ApiError::not_found(format!("no version {version} of `{name}`")).into_response(),
    }
}

// ---- publish -----------------------------------------------------------------

/// `PUT {api}/v1/packages/<name>/<version>`: `manifest` and `archive`.
pub async fn publish(
    State(app): State<Shared>,
    Path((name, version)): Path<(String, String)>,
    headers: HeaderMap,
    multipart: std::result::Result<Multipart, MultipartRejection>,
) -> Result<Response> {
    // (checked now, answered after the upload is read: a client still
    // sending when the connection closes sees it aborted on Windows, not
    // the answer; decision T552)
    let checked = user(&app, &headers).and_then(|login| {
        check_name(&name)?;
        if semver::Version::parse(&version).is_err() {
            return Err(ApiError::bad(
                "invalid-version",
                format!("`{version}` is not a SemVer version"),
            ));
        }
        Ok(login)
    });
    let max = app.config.max_archive;
    let declared = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok());
    if declared.is_some_and(|n| n > max + 1024 * 1024) {
        return Err(ApiError::too_large(max));
    }
    let mut multipart = multipart.map_err(|e| {
        ApiError::bad(
            "invalid-request",
            format!("a publish is `multipart/form-data` with `manifest` and `archive`: {e}"),
        )
    })?;
    let login = match checked {
        Ok(login) => login,
        Err(e) => {
            // the upload read (its size is capped) and dropped
            while let Ok(Some(field)) = multipart.next_field().await {
                if field.bytes().await.is_err() {
                    break;
                }
            }
            return Err(e);
        }
    };
    let mut manifest: Option<String> = None;
    let mut archive: Option<Vec<u8>> = None;
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) if e.status() == StatusCode::PAYLOAD_TOO_LARGE => {
                return Err(ApiError::too_large(max));
            }
            Err(e) => return Err(ApiError::bad("invalid-request", e.body_text())),
        };
        let field_name = field.name().unwrap_or_default().to_string();
        let bytes = match field.bytes().await {
            Ok(b) => b,
            Err(e) if e.status() == StatusCode::PAYLOAD_TOO_LARGE => {
                return Err(ApiError::too_large(max));
            }
            Err(e) => return Err(ApiError::bad("invalid-request", e.body_text())),
        };
        match field_name.as_str() {
            "manifest" => manifest = Some(String::from_utf8_lossy(&bytes).into_owned()),
            "archive" => {
                if bytes.len() > max {
                    return Err(ApiError::too_large(max));
                }
                archive = Some(bytes.to_vec());
            }
            _ => {}
        }
    }
    let (Some(manifest), Some(archive)) = (manifest, archive) else {
        return Err(ApiError::bad(
            "invalid-request",
            "a publish needs the fields `manifest` and `archive`",
        ));
    };
    let entry = blocking({
        let app = app.clone();
        move || publish_blocking(&app, &name, &version, &login, &manifest, &archive)
    })
    .await?;
    Ok(json(
        StatusCode::CREATED,
        serde_json::json!({ "name": entry.name, "vers": entry.vers, "cksum": entry.cksum }),
    ))
}

fn publish_blocking(
    app: &Shared,
    name: &str,
    version: &str,
    login: &str,
    manifest: &str,
    archive: &[u8],
) -> Result<IndexEntry> {
    let checked = check_manifest(manifest, name, version, &app.config.public_url)?;
    let files = registry::read_archive(archive).map_err(|e| match e {
        registry::ArchiveError::TooLarge(why) => {
            ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "too-large", why)
        }
        other => ApiError::bad("invalid-archive", other.to_string()),
    })?;
    let Some(inner) = files.get(wowlua_manifest::MANIFEST_FILE) else {
        return Err(ApiError::bad(
            "invalid-archive",
            "the archive has no `wowlua.json` at its root",
        ));
    };
    let inner: serde_json::Value = serde_json::from_slice(inner).map_err(|e| {
        ApiError::bad(
            "invalid-archive",
            format!("the archive's `wowlua.json`: {e}"),
        )
    })?;
    if inner.pointer("/package/name").and_then(|v| v.as_str()) != Some(name)
        || inner.pointer("/package/version").and_then(|v| v.as_str()) != Some(version)
    {
        return Err(ApiError::bad(
            "manifest-mismatch",
            format!("the archive's `wowlua.json` is not {name} {version}"),
        ));
    }
    let admin = app.config.is_admin(login);
    let new = app.store.entries(name).is_empty();
    if new && let Some(reserved) = app.store.reserved().get(name) {
        let author = reserved.authors.iter().any(|a| same_login(a, login));
        if !author && !admin {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "reserved",
                format!(
                    "`{name}` is reserved for the authors of {}; if you are one of them, claim it: {}",
                    if reserved.note.is_empty() {
                        name
                    } else {
                        &reserved.note
                    },
                    crate::pages::claim_url(name)
                ),
            ));
        }
    }
    let meta = checked.meta;
    let version_meta = crate::store::VersionMeta::from_files(&files, crate::auth::now());
    let entry = IndexEntry {
        name: name.to_string(),
        vers: version.to_string(),
        deps: checked.deps,
        cksum: registry::checksum(&files),
        features: checked.features,
        yanked: false,
        wowlua: checked.wowlua,
    };
    match app
        .store
        .publish(&entry, archive, login, admin, &meta, &version_meta)
    {
        Ok(()) => Ok(entry),
        Err(PublishError::Exists) => Err(ApiError::new(
            StatusCode::CONFLICT,
            "version-exists",
            format!(
                "{name} {version} exists; a published version is never replaced (publish a new version, or yank this one)"
            ),
        )),
        Err(PublishError::NotOwner) => Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "not-owner",
            format!("`{login}` is not an owner of `{name}`"),
        )),
        Err(PublishError::Io(e)) => Err(ApiError::internal(e)),
    }
}

/// What the index line and search take from a manifest.
struct Checked {
    deps: Vec<IndexDep>,
    features: BTreeMap<String, Vec<String>>,
    wowlua: Option<String>,
    meta: crate::store::Meta,
}

/// Checks a published manifest: valid by `wowlua-manifest`'s rules (of
/// this server's version; a package requiring another compiler version is
/// checked for the fields below only), the URL's name and version, a
/// package kind the registry takes, and dependencies only from registries.
fn check_manifest(text: &str, name: &str, version: &str, own_registry: &str) -> Result<Checked> {
    let raw: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| ApiError::bad("invalid-manifest", format!("`manifest` is not JSON: {e}")))?;
    // Said first: a git dependency may be valid for wowlua, but not here.
    if let Some(map) = raw["dependencies"].as_object() {
        for (dep, value) in map {
            if let Some(source) = ["git", "svn", "path", "archive"]
                .iter()
                .find(|k| value.get(**k).is_some())
            {
                return Err(ApiError::bad(
                    "dependency-source",
                    format!(
                        "E4231: dependency `{dep}` comes from a `{source}` source; a registry package depends on registry packages only"
                    ),
                ));
            }
        }
    }
    let compiler =
        semver::Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(semver::Version::new(0, 0, 0));
    let wowlua = raw
        .pointer("/package/wowlua")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let gate = wowlua
        .as_deref()
        .map(|r| semver::VersionReq::parse(r).map(|q| q.matches(&compiler)));
    if !matches!(gate, Some(Ok(false))) {
        let mut db = wowlua_span::SourceDb::new();
        let output = wowlua_manifest::parse(
            &mut db,
            std::path::Path::new("wowlua.json"),
            text.to_string(),
            &compiler,
        );
        let errors: Vec<String> = output
            .diagnostics
            .iter()
            .filter(|d| d.is_error())
            .map(|d| match d.code {
                Some(code) => format!("{code}: {}", d.message),
                None => d.message.clone(),
            })
            .collect();
        if !errors.is_empty() {
            return Err(ApiError::bad("invalid-manifest", errors.join("; ")));
        }
    }
    let package = &raw["package"];
    if package["name"].as_str() != Some(name) || package["version"].as_str() != Some(version) {
        return Err(ApiError::bad(
            "manifest-mismatch",
            format!(
                "the URL names {name} {version}, the manifest {} {}",
                package["name"].as_str().unwrap_or("no package"),
                package["version"].as_str().unwrap_or("")
            ),
        ));
    }
    match package["kind"].as_str().unwrap_or("addon") {
        "library" | "lua-library" | "types" => {}
        "addon" => {
            return Err(ApiError::bad(
                "addon",
                "E4230: addons cannot be published to the wowlua registry; it hosts `library`, `lua-library` and `types` packages",
            ));
        }
        other => {
            return Err(ApiError::bad(
                "invalid-manifest",
                format!("unknown package kind `{other}`"),
            ));
        }
    }
    let mut deps = Vec::new();
    if let Some(map) = raw["dependencies"].as_object() {
        for (dep, value) in map {
            let (req, registry_url, optional) = match value {
                serde_json::Value::String(r) => (r.clone(), None, false),
                serde_json::Value::Object(o) => {
                    let registry_url = match o.get("registry").and_then(|r| r.as_str()) {
                        Some(r) if !r.contains("://") => {
                            return Err(ApiError::bad(
                                "invalid-manifest",
                                format!(
                                    "dependency `{dep}` names the registry `{r}`; a published manifest names registries by URL"
                                ),
                            ));
                        }
                        Some(r) => Some(registry::normalize_url(r))
                            .filter(|r| r != &registry::normalize_url(own_registry)),
                        None => None,
                    };
                    (
                        o.get("version")
                            .and_then(|v| v.as_str())
                            .unwrap_or("*")
                            .to_string(),
                        registry_url,
                        o.get("optional").and_then(|v| v.as_bool()).unwrap_or(false),
                    )
                }
                _ => {
                    return Err(ApiError::bad(
                        "invalid-manifest",
                        format!("dependency `{dep}` is neither a requirement nor an object"),
                    ));
                }
            };
            if semver::VersionReq::parse(&req).is_err() {
                return Err(ApiError::bad(
                    "invalid-manifest",
                    format!("dependency `{dep}`: `{req}` is not a version requirement"),
                ));
            }
            deps.push(IndexDep {
                name: dep.clone(),
                req,
                kind: "normal".into(),
                registry: registry_url,
                optional,
            });
        }
    }
    let features = raw["features"]
        .as_object()
        .map(|m| {
            m.iter()
                .map(|(k, v)| {
                    let list = v
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| x.as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_default();
                    (k.clone(), list)
                })
                .collect()
        })
        .unwrap_or_default();
    let meta = crate::store::Meta {
        vers: version.to_string(),
        description: package["description"].as_str().unwrap_or("").to_string(),
        keywords: package["keywords"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|k| k.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        kind: package["kind"].as_str().unwrap_or("").to_string(),
    };
    Ok(Checked {
        deps,
        features,
        wowlua,
        meta,
    })
}

// ---- yank ------------------------------------------------------------------

async fn set_yanked(
    app: Shared,
    name: String,
    version: String,
    headers: HeaderMap,
    yanked: bool,
) -> Result<Response> {
    let login = user(&app, &headers)?;
    check_name(&name)?;
    let done = blocking(move || -> Result<()> {
        check_owner(&app, &name, &login)?;
        match app.store.set_yanked(&name, &version, yanked) {
            Ok(true) => Ok(()),
            Ok(false) => Err(ApiError::not_found(format!(
                "no version {version} of `{name}`"
            ))),
            Err(e) => Err(ApiError::internal(e)),
        }
    })
    .await;
    done.map(|()| json(StatusCode::OK, serde_json::json!({ "ok": true })))
}

/// `DELETE {api}/v1/packages/<name>/<version>`.
pub async fn yank(
    State(app): State<Shared>,
    Path((name, version)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response> {
    set_yanked(app, name, version, headers, true).await
}

/// `PUT {api}/v1/packages/<name>/<version>/unyank`.
pub async fn unyank(
    State(app): State<Shared>,
    Path((name, version)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response> {
    set_yanked(app, name, version, headers, false).await
}

// ---- owners ------------------------------------------------------------------

#[derive(serde::Deserialize)]
pub struct Users {
    pub users: Vec<String>,
}

fn users_of(body: &[u8]) -> Result<Vec<String>> {
    let users: Users = serde_json::from_slice(body).map_err(|e| {
        ApiError::bad(
            "invalid-request",
            format!("the body is `{{\"users\": [GitHub logins]}}`: {e}"),
        )
    })?;
    if users.users.is_empty() {
        return Err(ApiError::bad("invalid-request", "`users` is empty"));
    }
    Ok(users.users)
}

/// `GET {api}/v1/packages/<name>/owners`.
pub async fn owners_list(State(app): State<Shared>, Path(name): Path<String>) -> Result<Response> {
    check_name(&name)?;
    let owners = blocking(move || {
        if app.store.entries(&name).is_empty() {
            None
        } else {
            Some(app.store.owners(&name))
        }
    })
    .await
    .ok_or_else(|| ApiError::not_found("no such package"))?;
    let users: Vec<serde_json::Value> = owners
        .iter()
        .map(|o| serde_json::json!({ "login": o }))
        .collect();
    Ok(json(StatusCode::OK, serde_json::json!({ "users": users })))
}

/// Adds GitHub users as owners: each must exist on GitHub.
pub fn add_owners(app: &Shared, name: &str, login: &str, users: &[String]) -> Result<Vec<String>> {
    check_owner(app, name, login)?;
    let mut canonical = Vec::new();
    for u in users {
        match app.config.github.user(u) {
            Ok(Some(c)) => canonical.push(c),
            Ok(None) => {
                return Err(ApiError::bad(
                    "unknown-user",
                    format!("GitHub has no user `{u}`"),
                ));
            }
            Err(e) => {
                return Err(ApiError::new(StatusCode::BAD_GATEWAY, "github", e));
            }
        }
    }
    let result = app
        .store
        .change_owners(name, |owners| {
            for c in &canonical {
                if !owners.iter().any(|o| same_login(o, c)) {
                    owners.push(c.clone());
                }
            }
            Ok(owners.clone())
        })
        .map_err(ApiError::internal)?;
    result.map_err(|e| ApiError::bad("invalid-request", e))
}

/// Removes owners; the last one stays.
pub fn remove_owners(
    app: &Shared,
    name: &str,
    login: &str,
    users: &[String],
) -> Result<Vec<String>> {
    check_owner(app, name, login)?;
    let result = app
        .store
        .change_owners(name, |owners| {
            let kept: Vec<String> = owners
                .iter()
                .filter(|o| !users.iter().any(|u| same_login(u, o)))
                .cloned()
                .collect();
            if kept.is_empty() {
                return Err("a package keeps at least one owner".to_string());
            }
            *owners = kept;
            Ok(owners.clone())
        })
        .map_err(ApiError::internal)?;
    result.map_err(|e| ApiError::bad("last-owner", e))
}

/// `PUT {api}/v1/packages/<name>/owners`.
pub async fn owners_add(
    State(app): State<Shared>,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response> {
    let login = user(&app, &headers)?;
    check_name(&name)?;
    let users = users_of(&body)?;
    let owners = blocking(move || add_owners(&app, &name, &login, &users)).await?;
    Ok(json(
        StatusCode::OK,
        serde_json::json!({ "ok": true, "users": owners }),
    ))
}

/// `DELETE {api}/v1/packages/<name>/owners`.
pub async fn owners_remove(
    State(app): State<Shared>,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response> {
    let login = user(&app, &headers)?;
    check_name(&name)?;
    let users = users_of(&body)?;
    let owners = blocking(move || remove_owners(&app, &name, &login, &users)).await?;
    Ok(json(
        StatusCode::OK,
        serde_json::json!({ "ok": true, "users": owners }),
    ))
}

// ---- reserved names ------------------------------------------------------------

/// `GET {api}/v1/reserved`.
pub async fn reserved_list(State(app): State<Shared>) -> Response {
    let reserved = blocking(move || app.store.reserved()).await;
    let list: Vec<serde_json::Value> = reserved
        .iter()
        .map(|(n, r)| serde_json::json!({ "name": n, "authors": r.authors, "note": r.note }))
        .collect();
    json(StatusCode::OK, serde_json::json!({ "reserved": list }))
}

fn admin(app: &Shared, headers: &HeaderMap) -> Result<String> {
    let login = user(app, headers)?;
    if app.config.is_admin(&login) {
        Ok(login)
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "not-admin",
            "only the registry's admins manage reserved names",
        ))
    }
}

#[derive(serde::Deserialize)]
pub struct ReservedBody {
    #[serde(default)]
    pub users: Vec<String>,
    #[serde(default)]
    pub note: String,
}

/// `PUT {api}/v1/reserved/<name>` (admins): reserves a name for the
/// `users`, or assigns it to them.
pub async fn reserved_set(
    State(app): State<Shared>,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response> {
    admin(&app, &headers)?;
    check_name(&name)?;
    let body: ReservedBody = serde_json::from_slice(&body).map_err(|e| {
        ApiError::bad(
            "invalid-request",
            format!("the body is `{{\"users\": [GitHub logins], \"note\": \"…\"}}`: {e}"),
        )
    })?;
    blocking(move || {
        let note = if body.note.is_empty() {
            app.store
                .reserved()
                .get(&name)
                .map(|r| r.note.clone())
                .unwrap_or_default()
        } else {
            body.note
        };
        app.store.set_reserved(
            &name,
            Some(Reserved {
                authors: body.users,
                note,
                released: false,
            }),
        )
    })
    .await
    .map_err(ApiError::internal)?;
    Ok(json(StatusCode::OK, serde_json::json!({ "ok": true })))
}

/// `DELETE {api}/v1/reserved/<name>` (admins): the name is free again.
pub async fn reserved_release(
    State(app): State<Shared>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Result<Response> {
    admin(&app, &headers)?;
    check_name(&name)?;
    blocking(move || app.store.set_reserved(&name, None))
        .await
        .map_err(ApiError::internal)?;
    Ok(json(StatusCode::OK, serde_json::json!({ "ok": true })))
}

// ---- search --------------------------------------------------------------------

/// How well a package matches one word of a search: 0 is no match.
fn word_score(word: &str, name: &str, meta: &crate::store::Meta) -> u32 {
    word_match(word, name, &meta.keywords, &meta.description).0
}

/// What of a package one search word matches, the search's way: the
/// score (0: no match), and whether the name, a keyword, the description.
pub fn word_match(
    word: &str,
    name: &str,
    keywords: &[String],
    description: &str,
) -> (u32, [bool; 3]) {
    let named = if name == word {
        100
    } else if name.starts_with(word) {
        60
    } else if name.contains(word) {
        40
    } else {
        0
    };
    let mut keyword = 0;
    for k in keywords.iter().map(|k| k.to_lowercase()) {
        keyword = keyword.max(if k == word {
            30
        } else if k.contains(word) {
            15
        } else {
            0
        });
    }
    let described = if description.to_lowercase().contains(word) {
        10
    } else {
        0
    };
    (
        named + keyword + described,
        [named > 0, keyword > 0, described > 0],
    )
}

/// A page of search results (spec §10.10.2).
pub fn search_packages(
    app: &Shared,
    query: &str,
    page: usize,
    per_page: usize,
) -> serde_json::Value {
    let words: Vec<String> = query
        .split_whitespace()
        .take(10)
        .map(|w| w.to_lowercase().chars().take(64).collect())
        .collect();
    let mut hits: Vec<(u32, String, String, crate::store::Meta)> = Vec::new();
    for name in app.store.packages() {
        let newest = app
            .store
            .entries(&name)
            .iter()
            .filter(|e| !e.yanked)
            .filter_map(IndexEntry::version)
            .max();
        let Some(newest) = newest else {
            continue;
        };
        let meta = app.store.meta(&name);
        let mut total = 0;
        let mut all = true;
        for w in &words {
            let s = word_score(w, &name, &meta);
            if s == 0 {
                all = false;
                break;
            }
            total += s;
        }
        if all {
            hits.push((total, name, newest.to_string(), meta));
        }
    }
    hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let total = hits.len();
    let packages: Vec<serde_json::Value> = hits
        .into_iter()
        .skip((page - 1) * per_page)
        .take(per_page)
        .map(|(score, name, vers, meta)| {
            serde_json::json!({
                "name": name,
                "vers": vers,
                "description": meta.description,
                "keywords": meta.keywords,
                "kind": meta.kind,
                "score": score,
            })
        })
        .collect();
    serde_json::json!({
        "packages": packages,
        "total": total,
        "page": page,
        "per_page": per_page,
    })
}

/// `GET {api}/v1/search?q=<words>&page=<n>&per_page=<n>`.
pub async fn search(
    State(app): State<Shared>,
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Response> {
    let number = |key: &str, default: usize| -> Result<usize> {
        match query.get(key) {
            None => Ok(default),
            Some(v) => v.parse::<usize>().ok().filter(|n| *n >= 1).ok_or_else(|| {
                ApiError::bad("invalid-request", format!("`{key}` is a number from 1"))
            }),
        }
    };
    let page = number("page", 1)?;
    let per_page = number("per_page", 20)?.min(100);
    let q = query.get("q").cloned().unwrap_or_default();
    let result = blocking(move || search_packages(&app, &q, page, per_page)).await;
    Ok(json(StatusCode::OK, result))
}
