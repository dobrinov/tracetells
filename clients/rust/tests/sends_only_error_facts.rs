//! Against a stand-in for Trace: what the client sends from a real axum app,
//! and what it must never send.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::routing::{get, post};
use tower::ServiceExt;

async fn boom() -> &'static str {
    panic!("token sk_live_51HkT9a2eZvKYlo2C0jw8Ln leaked into a panic")
}

#[tokio::test(flavor = "multi_thread")]
async fn reports_errors_and_panics_without_request_data() {
    // A stand-in for Trace that keeps what it receives.
    let received: Arc<Mutex<Vec<(String, serde_json::Value)>>> = Arc::default();
    let sink = received.clone();
    let trace = Router::new().route(
        "/v1/errors",
        post(move |headers: HeaderMap, body: String| {
            let sink = sink.clone();
            async move {
                let auth = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
                sink.lock().unwrap().push((auth, serde_json::from_str(&body).unwrap()));
                StatusCode::ACCEPTED
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, trace).await.unwrap() });

    let _guard = tracetells::init(
        tracetells::Config::default().key("tse_test").endpoint(format!("http://{addr}")).environment("test").release("v1.2.3"),
    );

    // The app: one handler fails, one panics.
    let app = Router::new()
        .route("/orders/{id}", get(|| async { (StatusCode::INTERNAL_SERVER_ERROR, "card 4111 1111 1111 1111 for anna@example.com") }))
        .route("/boom", get(boom))
        .layer(tracetells::TraceLayer);
    let secret_request = Request::get("/orders/8817?coupon=SECRETCOUPON")
        .header("cookie", "session=SECRETCOOKIE")
        .header("authorization", "Bearer SECRETBEARER")
        .body(Body::from("password=SECRETBODY"))
        .unwrap();
    let res = app.clone().oneshot(secret_request).await.unwrap();
    assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let panicked = tokio::spawn(app.oneshot(Request::get("/boom").body(Body::empty()).unwrap())).await;
    assert!(panicked.is_err(), "the panic still happens as it would without the client");

    // A handled error.
    let missing = std::fs::read_to_string("/definitely/not/here.toml").unwrap_err();
    tracetells::report(&missing);

    tracetells::flush(Duration::from_secs(3));
    let got = received.lock().unwrap().clone();
    let errors: Vec<serde_json::Value> = got.iter().flat_map(|(_, b)| b["errors"].as_array().unwrap().clone()).collect();
    assert!(got.iter().all(|(auth, _)| auth == "Bearer tse_test"));

    let http = errors.iter().find(|e| e["class"] == "HTTP 500").expect("the 500 response");
    assert_eq!((http["route"].as_str(), http["method"].as_str(), http["status"].as_i64()), (Some("/orders/:id"), Some("GET"), Some(500)));
    assert_eq!((http["environment"].as_str(), http["release"].as_str()), (Some("test"), Some("v1.2.3")));
    assert_eq!(http["runtime"]["name"], "rust");

    let panic = errors.iter().find(|e| e["class"] == "panic").expect("the panic");
    assert_eq!(panic["route"], "/boom");
    assert_eq!(panic["message"], "token [token] leaked into a panic");
    assert!(panic["backtrace"].as_array().unwrap().iter().any(|f| f["in_app"] == true));

    let io = errors.iter().find(|e| e["class"] == "io::Error").unwrap_or_else(|| panic!("the reported io error: {errors:?}"));
    assert!(io["message"].as_str().unwrap().contains("No such file"));

    let everything = serde_json::to_string(&got.iter().map(|(_, b)| b).collect::<Vec<_>>()).unwrap();
    for secret in ["SECRETCOUPON", "SECRETCOOKIE", "SECRETBEARER", "SECRETBODY", "4111", "anna@example.com", "sk_live", "/Users/", "/home/"] {
        assert!(!everything.contains(secret), "{secret} was sent: {everything}");
    }
    for key in ["headers", "cookies", "params", "body", "user", "context"] {
        assert!(errors.iter().all(|e| e.get(key).is_none()), "a {key} field was sent");
    }
}
