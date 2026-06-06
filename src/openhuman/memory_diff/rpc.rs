//! RPC request/response types and handler implementations.

use serde::{Deserialize, Serialize};

use crate::openhuman::config::rpc as config_rpc;
use crate::rpc::RpcOutcome;

use super::ops;
use super::store;
use super::types::*;

// ── Request / Response types ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct TakeSnapshotRequest {
    pub source_id: String,
}

#[derive(Debug, Serialize)]
pub struct TakeSnapshotResponse {
    pub snapshot: Snapshot,
}

#[derive(Debug, Deserialize)]
pub struct ListSnapshotsRequest {
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub limit: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct ListSnapshotsResponse {
    pub snapshots: Vec<Snapshot>,
}

#[derive(Debug, Deserialize)]
pub struct DiffRequest {
    #[serde(default)]
    pub from_snapshot_id: Option<String>,
    pub to_snapshot_id: String,
    #[serde(default)]
    pub include_text_diff: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct DiffResponse {
    pub diff: DiffResult,
}

#[derive(Debug, Deserialize)]
pub struct DiffSinceLastRequest {
    pub source_id: String,
    #[serde(default)]
    pub include_text_diff: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct DiffSinceLastResponse {
    pub diff: DiffResult,
}

#[derive(Debug, Deserialize)]
pub struct CreateCheckpointRequest {
    pub label: String,
}

#[derive(Debug, Serialize)]
pub struct CreateCheckpointResponse {
    pub checkpoint: Checkpoint,
}

#[derive(Debug, Deserialize)]
pub struct ListCheckpointsRequest {
    #[serde(default)]
    pub limit: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct ListCheckpointsResponse {
    pub checkpoints: Vec<Checkpoint>,
}

#[derive(Debug, Deserialize)]
pub struct DiffSinceCheckpointRequest {
    pub checkpoint_id: String,
    #[serde(default)]
    pub include_text_diff: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct DiffSinceCheckpointResponse {
    pub diff: CrossSourceDiff,
}

#[derive(Debug, Deserialize)]
pub struct CleanupRequest {
    pub older_than_days: u64,
}

#[derive(Debug, Serialize)]
pub struct CleanupResponse {
    pub deleted_snapshots: u64,
}

// ── Handlers ──────────────────────────────────────────────────────────

pub async fn take_snapshot_rpc(
    req: TakeSnapshotRequest,
) -> Result<RpcOutcome<TakeSnapshotResponse>, String> {
    let config = config_rpc::load_config_with_timeout().await?;
    let source = crate::openhuman::memory_sources::get_source(&req.source_id)
        .await?
        .ok_or_else(|| format!("source not found: {}", req.source_id))?;

    let snapshot = ops::take_snapshot(&source, &config, SnapshotTrigger::Manual).await?;
    Ok(RpcOutcome::new(
        TakeSnapshotResponse { snapshot },
        vec![],
    ))
}

pub async fn list_snapshots_rpc(
    req: ListSnapshotsRequest,
) -> Result<RpcOutcome<ListSnapshotsResponse>, String> {
    let config = config_rpc::load_config_with_timeout().await?;
    let workspace_dir = config.workspace_dir.clone();
    let limit = req.limit.unwrap_or(50) as u32;
    let source_id = req.source_id;

    let snapshots = tokio::task::spawn_blocking(move || {
        store::with_connection(&workspace_dir, |conn| {
            store::list_snapshots(conn, source_id.as_deref(), limit)
        })
    })
    .await
    .map_err(|e| format!("list_snapshots join: {e}"))?
    .map_err(|e: anyhow::Error| format!("list_snapshots: {e:#}"))?;

    Ok(RpcOutcome::new(
        ListSnapshotsResponse { snapshots },
        vec![],
    ))
}

pub async fn diff_rpc(req: DiffRequest) -> Result<RpcOutcome<DiffResponse>, String> {
    let config = config_rpc::load_config_with_timeout().await?;
    let diff = ops::compute_diff(
        &config,
        req.from_snapshot_id.as_deref(),
        &req.to_snapshot_id,
        req.include_text_diff.unwrap_or(false),
    )
    .await?;
    Ok(RpcOutcome::new(DiffResponse { diff }, vec![]))
}

pub async fn diff_since_last_rpc(
    req: DiffSinceLastRequest,
) -> Result<RpcOutcome<DiffSinceLastResponse>, String> {
    let config = config_rpc::load_config_with_timeout().await?;
    let source = crate::openhuman::memory_sources::get_source(&req.source_id)
        .await?
        .ok_or_else(|| format!("source not found: {}", req.source_id))?;

    let diff = ops::diff_since_last(&source, &config, req.include_text_diff.unwrap_or(false))
        .await?;
    Ok(RpcOutcome::new(DiffSinceLastResponse { diff }, vec![]))
}

pub async fn create_checkpoint_rpc(
    req: CreateCheckpointRequest,
) -> Result<RpcOutcome<CreateCheckpointResponse>, String> {
    let config = config_rpc::load_config_with_timeout().await?;
    let checkpoint = ops::create_checkpoint(&req.label, &config).await?;
    Ok(RpcOutcome::new(
        CreateCheckpointResponse { checkpoint },
        vec![],
    ))
}

pub async fn list_checkpoints_rpc(
    req: ListCheckpointsRequest,
) -> Result<RpcOutcome<ListCheckpointsResponse>, String> {
    let config = config_rpc::load_config_with_timeout().await?;
    let workspace_dir = config.workspace_dir.clone();
    let limit = req.limit.unwrap_or(20) as u32;

    let checkpoints = tokio::task::spawn_blocking(move || {
        store::with_connection(&workspace_dir, |conn| {
            store::list_checkpoints(conn, limit)
        })
    })
    .await
    .map_err(|e| format!("list_checkpoints join: {e}"))?
    .map_err(|e: anyhow::Error| format!("list_checkpoints: {e:#}"))?;

    Ok(RpcOutcome::new(
        ListCheckpointsResponse { checkpoints },
        vec![],
    ))
}

pub async fn diff_since_checkpoint_rpc(
    req: DiffSinceCheckpointRequest,
) -> Result<RpcOutcome<DiffSinceCheckpointResponse>, String> {
    let config = config_rpc::load_config_with_timeout().await?;
    let diff = ops::diff_since_checkpoint(
        &req.checkpoint_id,
        &config,
        req.include_text_diff.unwrap_or(false),
    )
    .await?;
    Ok(RpcOutcome::new(
        DiffSinceCheckpointResponse { diff },
        vec![],
    ))
}

pub async fn cleanup_rpc(
    req: CleanupRequest,
) -> Result<RpcOutcome<CleanupResponse>, String> {
    let config = config_rpc::load_config_with_timeout().await?;
    let deleted = ops::cleanup(&config, req.older_than_days as u32).await?;
    Ok(RpcOutcome::new(
        CleanupResponse {
            deleted_snapshots: deleted,
        },
        vec![],
    ))
}
