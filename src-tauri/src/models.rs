//! Getting Parakeet V3 onto the disk.
//!
//! One model, one URL, one place on disk: `<app-data>/models/`. The archive is
//! streamed to a `.part` file, checked against the published SHA-256, then
//! unpacked — so an interrupted download can never masquerade as a good model.
//!
//! The Silero VAD model does not live here: it is 1.7 MB and ships inside the
//! bundle as a Tauri resource, which keeps first run to a single download.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use talkie_shared::{events, ModelProgress, ModelStatus};
use tauri::{AppHandle, Emitter, Manager};

/// Handy's bucket. Personal use is fine; mirror this before distributing (see
/// the risks section of the implementation plan).
const MODEL_URL: &str = "https://blob.handy.computer/parakeet-v3-int8.tar.gz";
/// Published digest of the archive, from Handy's model catalog.
const MODEL_SHA256: &str = "43d37191602727524a7d8c6da0eef11c4ba24320f5b4730f1a2497befc2efa77";
/// Directory the archive extracts to, and the path `ParakeetModel::load` wants.
const MODEL_DIR_NAME: &str = "parakeet-tdt-0.6b-v3-int8";
/// One file that must exist for the extracted directory to count as complete.
const MODEL_SENTINEL: &str = "encoder-model.int8.onnx";

/// `<app-data>/models`, created on demand.
pub fn models_dir(app: &AppHandle) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .context("no app data directory")?
        .join("models");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Where `ParakeetModel::load` is pointed.
pub fn model_path(app: &AppHandle) -> Result<PathBuf> {
    Ok(models_dir(app)?.join(MODEL_DIR_NAME))
}

/// The Silero VAD weights that ship in the bundle.
pub fn vad_path(app: &AppHandle) -> Result<PathBuf> {
    app.path()
        .resolve(
            "resources/models/silero_vad_v4.onnx",
            tauri::path::BaseDirectory::Resource,
        )
        .context("could not resolve the bundled VAD model")
}

pub fn status(app: &AppHandle) -> ModelStatus {
    match model_path(app) {
        Ok(dir) if dir.join(MODEL_SENTINEL).is_file() => ModelStatus::Ready,
        _ => ModelStatus::Missing,
    }
}

fn emit(app: &AppHandle, progress: ModelProgress) {
    let _ = app.emit(events::MODEL_PROGRESS, progress);
}

/// Download, verify, and unpack the model, emitting `MODEL_PROGRESS` as it goes.
/// Returns early — and cheaply — if the model is already on disk.
pub async fn download(app: AppHandle) -> Result<()> {
    if status(&app) == ModelStatus::Ready {
        emit(
            &app,
            ModelProgress {
                downloaded_bytes: 0,
                total_bytes: None,
                extracting: false,
                done: true,
                error: None,
            },
        );
        return Ok(());
    }

    let result = download_inner(&app).await;
    if let Err(e) = &result {
        emit(
            &app,
            ModelProgress {
                downloaded_bytes: 0,
                total_bytes: None,
                extracting: false,
                done: false,
                error: Some(e.to_string()),
            },
        );
    }
    result
}

async fn download_inner(app: &AppHandle) -> Result<()> {
    let dir = models_dir(app)?;
    let archive_path = dir.join("parakeet-v3-int8.tar.gz.part");

    let response = reqwest::get(MODEL_URL)
        .await
        .context("could not reach the model host")?
        .error_for_status()
        .context("the model host refused the download")?;

    let total_bytes = response.content_length();
    let mut downloaded_bytes = 0u64;
    let mut hasher = Sha256::new();
    let mut file = fs::File::create(&archive_path)?;
    let mut stream = response.bytes_stream();

    // One event per ~2 MB: enough for a smooth bar, few enough that the webview
    // is not woken thousands of times during a 456 MB download.
    const PROGRESS_STEP: u64 = 2 * 1024 * 1024;
    let mut next_report = PROGRESS_STEP;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("the download was interrupted")?;
        hasher.update(&chunk);
        file.write_all(&chunk)?;
        downloaded_bytes += chunk.len() as u64;

        if downloaded_bytes >= next_report {
            next_report = downloaded_bytes + PROGRESS_STEP;
            emit(
                app,
                ModelProgress {
                    downloaded_bytes,
                    total_bytes,
                    extracting: false,
                    done: false,
                    error: None,
                },
            );
        }
    }
    file.flush()?;
    drop(file);

    let digest = format!("{:x}", hasher.finalize());
    if digest != MODEL_SHA256 {
        let _ = fs::remove_file(&archive_path);
        return Err(anyhow!(
            "the downloaded model did not match its checksum; nothing was installed"
        ));
    }

    emit(
        app,
        ModelProgress {
            downloaded_bytes,
            total_bytes,
            extracting: true,
            done: false,
            error: None,
        },
    );

    // Unpack on a blocking thread: 456 MB of gzip would otherwise stall the
    // async runtime for seconds.
    let dir_for_extract = dir.clone();
    let archive_for_extract = archive_path.clone();
    tokio::task::spawn_blocking(move || extract(&archive_for_extract, &dir_for_extract))
        .await
        .context("the unpacking task panicked")??;

    let _ = fs::remove_file(&archive_path);

    if status(app) != ModelStatus::Ready {
        return Err(anyhow!(
            "the archive unpacked but `{MODEL_DIR_NAME}/{MODEL_SENTINEL}` is missing"
        ));
    }

    emit(
        app,
        ModelProgress {
            downloaded_bytes,
            total_bytes,
            extracting: false,
            done: true,
            error: None,
        },
    );
    Ok(())
}

/// Unpack into a scratch directory first, then swap it into place, so a failure
/// part-way through never leaves a half-model where `status` can find it.
fn extract(archive: &Path, dir: &Path) -> Result<()> {
    let staging = dir.join(".unpacking");
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;

    let file = fs::File::open(archive)?;
    let decoder = flate2::read::GzDecoder::new(file);
    tar::Archive::new(decoder)
        .unpack(&staging)
        .context("the model archive could not be unpacked")?;

    let unpacked = staging.join(MODEL_DIR_NAME);
    let source = if unpacked.is_dir() {
        unpacked
    } else {
        staging.clone()
    };

    let target = dir.join(MODEL_DIR_NAME);
    if target.exists() {
        fs::remove_dir_all(&target)?;
    }
    fs::rename(&source, &target)?;

    if staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    Ok(())
}
