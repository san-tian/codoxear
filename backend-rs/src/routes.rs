use crate::app_state::{epoch_now, AppState};
use crate::models::{EventKind, LiveEvent, SendMessagePayload, TranscriptEvent};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::stream::{self, Stream};
use serde_json::json;
use std::convert::Infallible;
use std::time::Duration;
use tokio::time::sleep;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/bootstrap", get(bootstrap))
        .route("/api/v1/messages/send", post(send_message))
        .route("/api/v1/events/stream", get(events))
        .with_state(state)
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "service": "codoxear-backend-rs" }))
}

async fn bootstrap(State(state): State<AppState>) -> Json<crate::models::BootstrapPayload> {
    let store = state.store.read().await;
    Json(store.bootstrap())
}

async fn send_message(
    State(state): State<AppState>,
    Json(payload): Json<SendMessagePayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let now = epoch_now();
    let user_event = TranscriptEvent {
        id: format!("evt-user-{now:.3}"),
        kind: EventKind::User,
        title: Some("Queued from composer".into()),
        body: payload.text.clone(),
        meta: Some("sent to Rust preview backend".into()),
        created_at: now,
    };

    let session_snapshot = {
        let mut store = state.store.write().await;
        let detail = store
            .session_details
            .get_mut(&payload.session_id)
            .ok_or((StatusCode::NOT_FOUND, "unknown session".to_string()))?;
        detail.transcript.push(user_event.clone());

        let session = store
            .sessions
            .iter_mut()
            .find(|session| session.id == payload.session_id)
            .ok_or((StatusCode::NOT_FOUND, "unknown session".to_string()))?;
        session.last_line = payload.text.clone();
        session.status = "running".into();
        session.updated_at = now;
        session.unread = 0;
        session.clone()
    };

    let _ = state.tx.send(LiveEvent::MessageCreated {
        session_id: payload.session_id.clone(),
        event: user_event,
        session: session_snapshot.clone(),
    });

    let spawned_state = state.clone();
    let spawned_session_id = payload.session_id.clone();
    let spawned_text = payload.text.clone();
    tokio::spawn(async move {
        sleep(Duration::from_millis(420)).await;
        let now = epoch_now();
        let assistant_event = TranscriptEvent {
            id: format!("evt-assistant-{now:.3}"),
            kind: EventKind::Assistant,
            title: Some("Preview assistant".into()),
            body: format!(
                "Captured the new message and rebroadcast it over SSE. Next step is to replace this simulated response with real broker/session plumbing.\n\nLast user request: {spawned_text}"
            ),
            meta: Some("synthetic assistant event".into()),
            created_at: now,
        };

        let session_snapshot = {
            let mut store = spawned_state.store.write().await;
            let Some(detail) = store.session_details.get_mut(&spawned_session_id) else {
                return;
            };
            detail.transcript.push(assistant_event.clone());
            let Some(session) = store.sessions.iter_mut().find(|session| session.id == spawned_session_id) else {
                return;
            };
            session.last_line = "Synthetic assistant response emitted from Rust preview backend.".into();
            session.status = "idle".into();
            session.updated_at = now;
            session.clone()
        };

        let _ = spawned_state.tx.send(LiveEvent::MessageCreated {
            session_id: spawned_session_id,
            event: assistant_event,
            session: session_snapshot,
        });
    });

    Ok(Json(json!({ "ok": true })))
}

async fn events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let receiver = state.tx.subscribe();
    let connected = stream::once(async move {
        let event = LiveEvent::Connected {
            created_at: epoch_now(),
        };
        Ok(Event::default().data(serde_json::to_string(&event).unwrap()))
    });
    let broadcast_stream = BroadcastStream::new(receiver).filter_map(|event| {
        match event {
            Ok(payload) => Some(Ok(Event::default().data(serde_json::to_string(&payload).unwrap()))),
            Err(_) => None,
        }
    });

    Sse::new(connected.chain(broadcast_stream)).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(10))
            .text("keep-alive"),
    )
}

#[cfg(test)]
mod tests {
    use super::router;
    use crate::app_state::build_state;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_route_responds() {
      let app = router(build_state());
      let response = app
          .oneshot(Request::builder().uri("/api/v1/health").body(Body::empty()).unwrap())
          .await
          .unwrap();
      assert_eq!(response.status(), StatusCode::OK);
    }
}
