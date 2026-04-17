use axum::{
    extract::Path,
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde_json::json;


/// Lists all known sessions across work directories.
#[tracing::instrument(level = "debug")]
pub async fn list_sessions() -> Json<serde_json::Value> {
    let sessions = crate::session::list_all().await;
    let items: Vec<serde_json::Value> = sessions
        .iter()
        .map(|s| {
            json!({
                "id": s.id,
                "title": s.title,
                "work_dir": s.work_dir.to_string_lossy().to_string(),
                "archived": s.state.archived,
                "updated_at": s.updated_at,
            })
        })
        .collect();
    Json(json!({ "sessions": items }))
}

/// Returns wire events for a specific session.
#[tracing::instrument(level = "debug")]
pub async fn get_wire_events(
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sessions = crate::session::list_all().await;
    let session = sessions.into_iter().find(|s| s.id == session_id);
    match session {
        Some(s) => {
            let records = s.wire_file.records();
            Ok(Json(json!({
                "session_id": session_id,
                "events": records,
            })))
        }
        None => Err(StatusCode::NOT_FOUND),
    }
}

/// Returns trace snapshots for recent sessions.
#[tracing::instrument(level = "debug")]
pub async fn list_traces() -> Json<serde_json::Value> {
    let sessions = crate::session::list_all().await;
    let traces: Vec<serde_json::Value> = sessions
        .into_iter()
        .take(50)
        .map(|s| {
            json!({
                "session_id": s.id,
                "title": s.title,
                "work_dir": s.work_dir.to_string_lossy().to_string(),
                "updated_at": s.updated_at,
                "event_count": s.wire_file.records().len(),
            })
        })
        .collect();
    Json(json!({ "traces": traces }))
}

/// Returns runtime metrics.
#[tracing::instrument(level = "debug")]
pub async fn metrics() -> Json<serde_json::Value> {
    let sessions = crate::session::list_all().await;
    let active_sessions = sessions.iter().filter(|s| !s.state.archived).count();
    Json(json!({
        "sessions_total": sessions.len(),
        "active_sessions": active_sessions,
        "archived_sessions": sessions.len() - active_sessions,
        "background_tasks_running": 0,
    }))
}

/// SPA fallback: returns a minimal HTML dashboard for the Vis server.
pub async fn spa_fallback() -> impl IntoResponse {
    let html = r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Kimi Vis</title>
<style>
  body { font-family: system-ui, sans-serif; margin: 2rem; background: #f7f7f7; color: #222; }
  h1 { margin-bottom: 0.5rem; }
  .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 1rem; margin: 1.5rem 0; }
  .card { background: #fff; padding: 1rem; border-radius: 8px; box-shadow: 0 1px 3px rgba(0,0,0,0.1); }
  .card h3 { margin: 0 0 0.5rem; font-size: 0.875rem; color: #666; text-transform: uppercase; }
  .card .value { font-size: 1.75rem; font-weight: 600; }
  table { width: 100%; border-collapse: collapse; background: #fff; border-radius: 8px; overflow: hidden; box-shadow: 0 1px 3px rgba(0,0,0,0.1); }
  th, td { padding: 0.75rem 1rem; text-align: left; border-bottom: 1px solid #eee; }
  th { background: #fafafa; font-weight: 500; }
  tr:hover { background: #fafafa; }
  a { color: #0066cc; text-decoration: none; }
  a:hover { text-decoration: underline; }
  .muted { color: #888; }
  pre { background: #f4f4f4; padding: 1rem; border-radius: 6px; overflow: auto; max-height: 400px; }
</style>
</head>
<body>
<h1>Kimi Vis Server</h1>
<div class="grid">
  <div class="card"><h3>Sessions</h3><div class="value" id="sessions-total">-</div></div>
  <div class="card"><h3>Active</h3><div class="value" id="sessions-active">-</div></div>
  <div class="card"><h3>Archived</h3><div class="value" id="sessions-archived">-</div></div>
</div>
<h2>Sessions</h2>
<table>
  <thead>
    <tr><th>ID</th><th>Title</th><th>Work Directory</th><th>Updated</th><th>Actions</th></tr>
  </thead>
  <tbody id="sessions-body">
    <tr><td colspan="5" class="muted">Loading…</td></tr>
  </tbody>
</table>
<div id="trace-view" style="margin-top:2rem; display:none;">
  <h2>Trace: <span id="trace-id" class="muted"></span></h2>
  <pre id="trace-content">Loading…</pre>
</div>
<script>
async function loadMetrics() {
  try {
    const res = await fetch('/api/metrics');
    const data = await res.json();
    document.getElementById('sessions-total').textContent = data.sessions_total ?? '-';
    document.getElementById('sessions-active').textContent = data.active_sessions ?? '-';
    document.getElementById('sessions-archived').textContent = data.archived_sessions ?? '-';
  } catch (e) { console.error(e); }
}
async function loadSessions() {
  try {
    const res = await fetch('/api/sessions');
    const data = await res.json();
    const tbody = document.getElementById('sessions-body');
    tbody.innerHTML = '';
    if (!data.sessions || data.sessions.length === 0) {
      tbody.innerHTML = '<tr><td colspan="5" class="muted">No sessions found.</td></tr>';
      return;
    }
    for (const s of data.sessions) {
      const tr = document.createElement('tr');
      tr.innerHTML = '<td>' + escapeHtml(s.id) + '</td>' +
        '<td>' + escapeHtml(s.title || '') + '</td>' +
        '<td>' + escapeHtml(s.work_dir || '') + '</td>' +
        '<td>' + (s.updated_at ? new Date(s.updated_at * 1000).toLocaleString() : '-') + '</td>' +
        '<td><a href="#" onclick="showTrace(\'' + escapeHtml(s.id) + '\'); return false;">View trace</a></td>';
      tbody.appendChild(tr);
    }
  } catch (e) { console.error(e); }
}
async function showTrace(id) {
  const view = document.getElementById('trace-view');
  const content = document.getElementById('trace-content');
  const label = document.getElementById('trace-id');
  view.style.display = 'block';
  label.textContent = id;
  content.textContent = 'Loading…';
  try {
    const res = await fetch('/api/sessions/' + encodeURIComponent(id) + '/wire');
    const data = await res.json();
    content.textContent = JSON.stringify(data, null, 2);
  } catch (e) {
    content.textContent = 'Failed to load trace: ' + e;
  }
  view.scrollIntoView({ behavior: 'smooth' });
}
function escapeHtml(text) {
  const div = document.createElement('div');
  div.textContent = text;
  return div.innerHTML;
}
loadMetrics();
loadSessions();
setInterval(() => { loadMetrics(); loadSessions(); }, 10000);
</script>
</body>
</html>"##;
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
        html,
    )
}

/// Returns statistics about sessions and traces.
#[tracing::instrument(level = "debug")]
pub async fn statistics() -> Json<serde_json::Value> {
    let sessions = crate::session::list_all().await;
    let active = sessions.iter().filter(|s| !s.state.archived).count();
    let archived = sessions.len() - active;
    let total_events: usize = sessions.iter().map(|s| s.wire_file.records().len()).sum();
    Json(serde_json::json!({
        "sessions_total": sessions.len(),
        "sessions_active": active,
        "sessions_archived": archived,
        "total_wire_events": total_events,
    }))
}

/// Returns system information.
#[tracing::instrument(level = "debug")]
pub async fn system_info() -> Json<serde_json::Value> {
    let env = crate::utils::environment::Environment::detect().await;
    Json(serde_json::json!({
        "os_kind": env.os_kind,
        "os_arch": env.os_arch,
        "os_version": env.os_version,
        "shell_name": env.shell_name,
        "shell_path": env.shell_path.to_string_lossy().to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn list_traces_returns_array() {
        let resp = list_traces().await;
        assert!(resp.0.get("traces").is_some());
    }

    #[tokio::test]
    async fn metrics_returns_counts() {
        let resp = metrics().await;
        assert!(resp.0.get("sessions_total").is_some());
    }
}
