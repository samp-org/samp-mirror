use axum::body::Body;
use axum::http::{Request, StatusCode};
use samp_mirror::api::{self, AppState};
use samp_mirror::db::{Db, InsertRemark};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower::ServiceExt;

fn test_state() -> (AppState, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let db = Db::open(path.to_str().unwrap());
    let state = AppState {
        db: Arc::new(Mutex::new(db)),
        chain: "TestChain".to_string(),
        ss58_prefix: 42,
        version: "2.0.0".to_string(),
    };
    (state, dir)
}

async fn body_json(response: axum::http::Response<Body>) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn test_health_endpoint() {
    let (state, _dir) = test_state();
    let app = api::router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["chain"], "TestChain");
    assert_eq!(json["ss58_prefix"], 42);
    assert_eq!(json["synced_to"], 0);
    assert_eq!(json["version"], "2.0.0");
}

#[tokio::test]
async fn test_channels_endpoint_empty() {
    let (state, _dir) = test_state();
    let app = api::router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/channels")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json, serde_json::json!([]));
}

#[tokio::test]
async fn test_channels_endpoint_with_data() {
    let (state, _dir) = test_state();
    state.db.lock().await.insert_channel(500, 2);
    let app = api::router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/channels")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["block"], 500);
    assert_eq!(arr[0]["index"], 2);
}

#[tokio::test]
async fn test_remarks_by_type() {
    let (state, _dir) = test_state();
    state.db.lock().await.insert_remark(&InsertRemark {
        block_number: 100,
        ext_index: 1,
        sender: "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
        content_type: 0x10,
        channel_block: None,
        channel_index: None,
    });
    let app = api::router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/remarks?type=0x10&after=0")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["block"], 100);
}

#[tokio::test]
async fn test_remarks_by_sender() {
    let (state, _dir) = test_state();
    let sender = "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY";
    state.db.lock().await.insert_remark(&InsertRemark {
        block_number: 200,
        ext_index: 3,
        sender,
        content_type: 0x11,
        channel_block: None,
        channel_index: None,
    });
    let app = api::router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri(&format!("/v1/remarks?sender={sender}&after=0"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["block"], 200);
    assert_eq!(arr[0]["index"], 3);
}

#[tokio::test]
async fn test_channel_messages_endpoint() {
    let (state, _dir) = test_state();
    {
        let db = state.db.lock().await;
        db.insert_channel(100, 2);
        db.insert_remark(&InsertRemark {
            block_number: 150,
            ext_index: 1,
            sender: "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
            content_type: 20,
            channel_block: Some(100),
            channel_index: Some(2),
        });
    }
    let app = api::router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/channels/100/2/messages?after=0")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["block"], 150);
    assert_eq!(arr[0]["index"], 1);
}

#[tokio::test]
async fn test_404_unknown_route() {
    let (state, _dir) = test_state();
    let app = api::router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/unknown")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_remarks_no_filter_returns_bad_request() {
    let (state, _dir) = test_state();
    let app = api::router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/remarks")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
