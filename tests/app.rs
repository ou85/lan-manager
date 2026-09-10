use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use homelab_manager::{
    auth,
    model::*,
    server::{self, AppState},
    storage::Store,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
fn snapshot() -> Snapshot {
    Snapshot {
        schema_version: 1,
        credentials: Credentials {
            username: "admin".into(),
            password_hash: auth::hash_password("test-only-password-123").unwrap(),
        },
        devices: vec![],
        subnets: vec![],
        ports: vec![],
    }
}
fn fixture() -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::create(&dir.path().join("lab.redb"), &snapshot()).unwrap();
    (dir, AppState::new(s, false))
}
async fn call(
    app: &axum::Router,
    method: &str,
    path: &str,
    data: Option<Value>,
    cookie: Option<&str>,
    csrf: Option<&str>,
) -> axum::response::Response {
    let mut r = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "localhost:8080");
    if let Some(c) = cookie {
        r = r.header("cookie", c);
    }
    if let Some(c) = csrf {
        r = r.header("x-csrf-token", c);
    }
    if data.is_some() {
        r = r.header("content-type", "application/json");
    }
    app.clone()
        .oneshot(
            r.body(Body::from(data.map(|d| d.to_string()).unwrap_or_default()))
                .unwrap(),
        )
        .await
        .unwrap()
}
async fn body(r: axum::response::Response) -> Value {
    serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap()
}
async fn login(app: &axum::Router) -> (String, String) {
    let r = call(
        app,
        "POST",
        "/api/login",
        Some(json!({"username":"admin","password":"test-only-password-123"})),
        None,
        None,
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);
    let c = r.headers()["set-cookie"].to_str().unwrap().to_owned();
    assert!(c.contains("HttpOnly"));
    assert!(c.contains("SameSite=Strict"));
    let c = c.split(';').next().unwrap().to_owned();
    let data = body(r).await;
    (c, data["csrf"].as_str().unwrap().to_owned())
}
fn device(id: &str, ip: &str, subnet: &str, parent: &str) -> Value {
    json!({"id":id,"name":"lab-node","type":"Server","ip":ip,"subnet_id":subnet,"parent":parent,"status":"In service"})
}
fn service_port(id: &str, port: u16) -> Value {
    json!({"id":id,"port":port,"protocol":"TCP","host":"127.0.0.1","service":"Home Lab Manager","access":"Tailscale","status":"Active"})
}
#[tokio::test]
async fn auth_csrf_crud_and_restart_persistence() {
    let (dir, state) = fixture();
    let app = server::app(state);
    assert_eq!(
        call(&app, "GET", "/api/inventory", None, None, None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let (cookie, csrf) = login(&app).await;
    let secret_free =
        body(call(&app, "GET", "/api/inventory", None, Some(&cookie), None).await).await;
    assert!(secret_free.get("credentials").is_none());
    let subnet = uuid::Uuid::new_v4().to_string();
    let id = uuid::Uuid::new_v4().to_string();
    let n = json!({"id":subnet,"name":"Servers","cidr":"10.0.3.80/28","gateway":"10.0.3.81"});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/subnets",
            Some(n.clone()),
            Some(&cookie),
            None
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/subnets",
            Some(n),
            Some(&cookie),
            Some(&csrf)
        )
        .await
        .status(),
        StatusCode::OK
    );
    let d = device(&id, "10.0.3.83", &subnet, "");
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/devices",
            Some(d),
            Some(&cookie),
            Some(&csrf)
        )
        .await
        .status(),
        StatusCode::OK
    );
    let port = uuid::Uuid::new_v4().to_string();
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/ports",
            Some(service_port(&port, 8088)),
            Some(&cookie),
            Some(&csrf)
        )
        .await
        .status(),
        StatusCode::OK
    );
    let checked = body(
        call(
            &app,
            "POST",
            &format!("/api/ports/{port}/check"),
            None,
            Some(&cookie),
            Some(&csrf),
        )
        .await,
    )
    .await;
    assert_eq!(checked["ports"][0]["check_status"], "Unreachable");
    let duplicate = device(&uuid::Uuid::new_v4().to_string(), "10.0.3.83", &subnet, "");
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/devices",
            Some(duplicate),
            Some(&cookie),
            Some(&csrf)
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/subnets/{subnet}"),
            None,
            Some(&cookie),
            Some(&csrf)
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    drop(app);
    let store = Store::open(&dir.path().join("lab.redb")).unwrap();
    assert_eq!(store.read().unwrap().devices.len(), 1);
    assert_eq!(store.read().unwrap().ports.len(), 1);
    let app = server::app(AppState::new(store, false));
    assert_eq!(
        call(&app, "GET", "/api/inventory", None, Some(&cookie), None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let (cookie, csrf) = login(&app).await;
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/devices/{id}"),
            None,
            Some(&cookie),
            Some(&csrf)
        )
        .await
        .status(),
        StatusCode::OK
    );
    let result = body(
        call(
            &app,
            "DELETE",
            &format!("/api/ports/{port}"),
            None,
            Some(&cookie),
            Some(&csrf),
        )
        .await,
    )
    .await;
    assert!(result["ports"].as_array().unwrap().is_empty());
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/subnets/{subnet}"),
            None,
            Some(&cookie),
            Some(&csrf)
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/logout",
            None,
            Some(&cookie),
            Some(&csrf)
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&app, "GET", "/api/inventory", None, Some(&cookie), None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
}
#[tokio::test]
async fn rejects_cross_origin_and_throttles_password_attempts() {
    let (_dir, state) = fixture();
    let app = server::app(state);
    let r = Request::builder()
        .method("POST")
        .uri("/api/login")
        .header("host", "localhost:8080")
        .header("origin", "https://evil.example")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"username":"admin","password":"wrong"}).to_string(),
        ))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(r).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    for _ in 0..5 {
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/login",
                Some(json!({"username":"admin","password":"wrong"})),
                None,
                None
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/login",
            Some(json!({"username":"admin","password":"wrong"})),
            None,
            None
        )
        .await
        .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
}
#[tokio::test]
async fn parent_deletion_detaches_children_and_cycles_rejected() {
    let (_dir, state) = fixture();
    let app = server::app(state);
    let (c, t) = login(&app).await;
    let a = uuid::Uuid::new_v4().to_string();
    let b = uuid::Uuid::new_v4().to_string();
    for d in [device(&a, "", "", ""), device(&b, "", "", &a)] {
        assert_eq!(
            call(&app, "POST", "/api/devices", Some(d), Some(&c), Some(&t))
                .await
                .status(),
            StatusCode::OK
        );
    }
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/devices",
            Some(device(&a, "", "", &b)),
            Some(&c),
            Some(&t)
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let result = body(
        call(
            &app,
            "DELETE",
            &format!("/api/devices/{a}"),
            None,
            Some(&c),
            Some(&t),
        )
        .await,
    )
    .await;
    assert_eq!(result["devices"][0]["parent"], "");
}
#[test]
fn cidr_validation_and_atomic_rollback() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(&dir.path().join("lab.redb"), &snapshot()).unwrap();
    let n = uuid::Uuid::new_v4().to_string();
    store
        .update(|s| {
            s.subnets.push(Subnet {
                id: n.clone(),
                name: "point to point".into(),
                cidr: "10.0.0.0/31".into(),
                gateway: "10.0.0.0".into(),
                vlan: Some(1),
            });
            Ok(())
        })
        .unwrap();
    for ip in ["10.0.0.0", "10.0.0.1"] {
        store
            .update(|s| {
                s.devices.push(
                    serde_json::from_value(device(&uuid::Uuid::new_v4().to_string(), ip, &n, ""))
                        .unwrap(),
                );
                Ok(())
            })
            .unwrap();
    }
    assert!(
        store
            .update(|s| {
                s.subnets.push(Subnet {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: "overlap".into(),
                    cidr: "10.0.0.0/24".into(),
                    gateway: "".into(),
                    vlan: None,
                });
                Ok(())
            })
            .is_err()
    );
    assert_eq!(store.read().unwrap().subnets.len(), 1);
    assert!(
        store
            .update(|s| {
                s.subnets[0].cidr = "10.0.0.1/24".into();
                Ok(())
            })
            .is_err()
    );
    assert_eq!(store.read().unwrap().subnets[0].cidr, "10.0.0.0/31");
    assert!(!usable(
        "10.0.3.80/28".parse().unwrap(),
        "10.0.3.95".parse().unwrap()
    ));
    assert!(usable(
        "10.0.0.1/32".parse().unwrap(),
        "10.0.0.1".parse().unwrap()
    ));
}
#[test]
fn password_salts_and_database_lock() {
    let first = auth::hash_password("another-test-password").unwrap();
    let second = auth::hash_password("another-test-password").unwrap();
    assert_ne!(first, second);
    assert!(auth::verify("another-test-password", &first));
    assert!(!auth::verify("wrong", &first));
    assert!(auth::hash_password("12345").is_err());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lab.redb");
    let store = Store::create(&path, &snapshot()).unwrap();
    assert!(Store::open(&path).is_err());
    drop(store);
    assert!(Store::open(&path).is_ok());
}
#[test]
fn backup_and_restore_commands() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    std::fs::create_dir(&data).unwrap();
    let path = data.join("homelab.redb");
    let original = snapshot();
    let store = Store::create(&path, &original).unwrap();
    drop(store);
    let backup = dir.path().join("backup.redb");
    let bin = env!("CARGO_BIN_EXE_homelab");
    assert!(
        std::process::Command::new(bin)
            .arg("--data-dir")
            .arg(&data)
            .arg("backup")
            .arg("--output")
            .arg(&backup)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        !std::process::Command::new(bin)
            .arg("--data-dir")
            .arg(&data)
            .arg("backup")
            .arg("--output")
            .arg(&backup)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        !std::process::Command::new(bin)
            .arg("--data-dir")
            .arg(&data)
            .arg("restore")
            .arg("--from")
            .arg(&backup)
            .status()
            .unwrap()
            .success()
    );
    let store = Store::open(&path).unwrap();
    store
        .update(|s| {
            s.credentials.password_hash = auth::hash_password("changed-test-password")?;
            Ok(())
        })
        .unwrap();
    drop(store);
    assert!(
        std::process::Command::new(bin)
            .arg("--data-dir")
            .arg(&data)
            .arg("restore")
            .arg("--from")
            .arg(&backup)
            .arg("--force")
            .status()
            .unwrap()
            .success()
    );
    let store = Store::open(&path).unwrap();
    assert_eq!(
        store.read().unwrap().credentials.password_hash,
        original.credentials.password_hash
    );
}
#[tokio::test]
async fn serves_embedded_ui_and_rejects_unknown_assets() {
    let (_d, state) = fixture();
    let app = server::app(state);
    let r = call(&app, "GET", "/", None, None, None).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert!(r.headers().contains_key("content-security-policy"));
    let b = r.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8(b.to_vec()).unwrap();
    assert!(html.contains("lang=\"en\""));
    assert_eq!(
        call(&app, "GET", "/missing.js", None, None, None)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}
