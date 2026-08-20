//! The speech-model section, shared by first run and settings.
//!
//! Onboarding is not the only way to get the model: it can be skipped, the
//! download can fail, and the model directory can be deleted out from under a
//! working install. So this lives in one component that both surfaces mount,
//! and it always reflects what is actually on disk rather than what onboarding
//! believes happened.

use leptos::prelude::*;
use leptos::task::spawn_local;
use talkie_shared::{commands, events, ModelProgress, ModelStatus};

use crate::ipc;

/// Status, a download button when one is needed, and progress while it runs.
///
/// `on_ready` fires when the model finishes installing — onboarding uses it to
/// advance a step; settings passes a no-op.
#[component]
pub fn ModelSection<F>(on_ready: F) -> impl IntoView
where
    F: Fn() + Copy + 'static,
{
    let status = RwSignal::new(None::<ModelStatus>);
    let progress = RwSignal::new(None::<ModelProgress>);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(String::new());

    Effect::new(move |_| {
        spawn_local(async move {
            match ipc::fetch::<ModelStatus>(commands::GET_MODEL_STATUS).await {
                Ok(current) => {
                    status.set(Some(current));
                    if current == ModelStatus::Ready {
                        on_ready();
                    }
                }
                Err(e) => error.set(e),
            }
        });
    });

    ipc::listen::<ModelProgress, _>(events::MODEL_PROGRESS, move |update| {
        if let Some(message) = update.error.clone() {
            error.set(message);
            busy.set(false);
            status.set(Some(ModelStatus::Missing));
        }
        if update.done {
            busy.set(false);
            status.set(Some(ModelStatus::Ready));
            on_ready();
        }
        progress.set(Some(update));
    });

    let download = move |_| {
        busy.set(true);
        error.set(String::new());
        status.set(Some(ModelStatus::Downloading));
        spawn_local(async move {
            // Progress arrives as events; this resolves only at the very end.
            if let Err(e) = ipc::fire(commands::DOWNLOAD_MODEL).await {
                error.set(e);
                busy.set(false);
                status.set(Some(ModelStatus::Missing));
            }
        });
    };

    let ready = move || status.get() == Some(ModelStatus::Ready);

    view! {
        <div class="model-section">
            <Show
                when=ready
                fallback=move || {
                    view! {
                        // COPY: onboarding.model.body
                        <p class="muted">
                            "The voice transcription model, Parakeet V3, runs locally, so your data
                            stays on your device."
                        </p>
                        <Show when=move || progress.get().is_some()>
                            <ProgressBar progress=progress />
                        </Show>
                        <div class="actions">
                            <span class="status error">{move || error.get()}</span>
                            <button
                                class="primary"
                                prop:disabled=move || busy.get()
                                on:click=download
                            >
                                // COPY: onboarding.model.cta
                                {move || if busy.get() { "Downloading…" } else { "Download Model" }}
                            </button>
                        </div>
                    }
                }
            >
                // COPY: onboarding.model.installed — placeholder
                <p class="muted">"Lorem ipsum: Parakeet V3, installed and offline."</p>
            </Show>
        </div>
    }
}

/// Bytes for the download, then an indeterminate sweep while the archive
/// unpacks — a step with no byte count but several seconds of wall time.
#[component]
fn ProgressBar(progress: RwSignal<Option<ModelProgress>>) -> impl IntoView {
    let fraction = move || progress.get().and_then(|p| p.fraction()).unwrap_or(0.0);
    let sweeping = move || progress.get().is_some_and(|p| p.extracting && !p.done);

    let label = move || {
        let Some(p) = progress.get() else {
            return String::new();
        };
        if p.done {
            // COPY: onboarding.model.ready
            return "Finished.".to_string();
        }
        if p.extracting {
            // COPY: onboarding.model.unpacking
            return "Unpacking…".to_string();
        }
        let mb = |bytes: u64| bytes as f64 / (1024.0 * 1024.0);
        match p.total_bytes {
            Some(total) => format!("{:.0} of {:.0} MB", mb(p.downloaded_bytes), mb(total)),
            None => format!("{:.0} MB", mb(p.downloaded_bytes)),
        }
    };

    view! {
        <div class="progress">
            <div class="progress-track" class:indeterminate=sweeping>
                <div
                    class="progress-fill"
                    style:width=move || format!("{:.1}%", fraction() * 100.0)
                ></div>
            </div>
            <span class="progress-label">{label}</span>
        </div>
    }
}
