use std::time::{Duration, Instant};

use gabriel_lib::core::engine::EngineState;
use gabriel_lib::core::settings;
use gabriel_lib::core::EngineConfig;
use gabriel_lib::ipc::commands::search_huggingface_models;
use gabriel_lib::types::ModelRuntimeInfo;

/// Live trace of the Add Model → Browse Hugging Face → search step.
/// Hits the real HF API (same command the Models page invokes).
#[tokio::test]
async fn hf_search_lists_tinyllama_gguf_files() {
    let t0 = Instant::now();
    let results = search_huggingface_models("tinyllama gguf".to_string())
        .await
        .expect("live HF search must succeed");
    println!("[hf-live] search took {:?}", t0.elapsed());
    assert!(
        !results.is_empty(),
        "expected at least one repo with gguf/safetensors files"
    );
    let first = &results[0];
    println!(
        "[hf-live] top repo: {} ({} compatible files)",
        first.id,
        first.files.len()
    );
    assert!(!first.files.is_empty());
    for f in first.files.iter().take(3) {
        println!("[hf-live]   file: {} size={:?}", f.name, f.size_bytes);
    }
}

/// Live trace of the row Add button path: stream → disk → register → refresh.
/// Uses a small real file (README.md) through the exact same
/// resolve/stream/register code path as a multi-GB GGUF (registration does
/// not validate weight content, only the copy + registry insert).
#[tokio::test]
async fn hf_streaming_download_registers_and_reports_progress() {
    let engine = EngineState::new(EngineConfig::default());
    let repo = "TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF".to_string();
    let filename = "README.md".to_string();
    let dir = settings::models_dir().expect("models dir must resolve");
    let dest = dir.join(&filename);
    let pre_existed = dest.exists();

    let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(u64, Option<u64>)>::new()));
    let events_clone = events.clone();
    let t0 = Instant::now();
    let info = engine
        .download_huggingface_model_with_progress(
            repo.clone(),
            filename.clone(),
            "llm".into(),
            move |downloaded, total| {
                events_clone.lock().unwrap().push((downloaded, total));
            },
        )
        .await
        .expect("live HF download must succeed");
    println!("[hf-live] download took {:?}", t0.elapsed());

    let meta = std::fs::metadata(&dest).expect("downloaded file must exist on disk");
    println!("[hf-live] bytes on disk: {}", meta.len());
    assert!(meta.len() > 0, "downloaded file must be non-empty");

    let models = engine.list_models();
    assert!(
        models.iter().any(|m| m.id == info.id),
        "registered model must appear in list_models (id={})",
        info.id
    );
    println!(
        "[hf-live] registered id: {} disk_bytes={} (models in registry: {})",
        info.id,
        info.disk_bytes,
        models.len()
    );

    let ev = events.lock().unwrap();
    println!("[hf-live] progress events: {}", ev.len());
    assert!(!ev.is_empty(), "must emit at least one progress event");
    let (last_dl, last_total) = ev.last().copied().unwrap();
    println!(
        "[hf-live] final progress: downloaded={} total={:?}",
        last_dl, last_total
    );
    assert_eq!(last_dl, meta.len(), "final progress must equal bytes on disk");
    assert!(
        ev.windows(2).all(|w| w[1].0 >= w[0].0),
        "progress must be monotonic"
    );

    if !pre_existed {
        std::fs::remove_file(&dest).expect("test cleanup must remove probe file");
        assert!(!dest.exists(), "probe file must be cleaned up");
        println!("[hf-live] probe file cleaned up");
    } else {
        println!("[hf-live] note: destination pre-existed, left in place");
    }
    assert!(
        !dir.join(format!("{filename}.part")).exists(),
        "no .part file may remain after success"
    );
}

/// Live trace of Cancel clicked mid-download on a real GGUF: the loop must
/// abort, the partial file must vanish, and nothing may be registered.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hf_cancel_mid_stream_aborts_and_cleans_up() {
    enum DlEv {
        Prog(u64, Option<u64>),
        Done(Result<Option<ModelRuntimeInfo>, String>),
    }
    let engine = EngineState::new(EngineConfig::default());
    // Cancelling an unknown id is a no-op false, never an error.
    assert!(!engine.cancel_hf_download("no-such-download"));

    let repo = "TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF".to_string();
    let filename = "tinyllama-1.1b-chat-v1.0.Q2_K.gguf".to_string();
    let dir = settings::models_dir().expect("models dir must resolve");
    let dest = dir.join(&filename);
    let part = dir.join(format!("{filename}.part"));
    // Preconditions: a previous session must not have left these behind
    // (startup cleanup covers .part; the final name should not exist yet).
    assert!(!dest.exists(), "final file must not pre-exist this test");

    let (tx, rx) = std::sync::mpsc::channel::<DlEv>();
    let tx_prog = tx.clone();
    let tx_done = tx.clone();
    let id = format!(
        "test-cancel-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    engine.start_hf_download(
        id.clone(),
        repo.clone(),
        filename.clone(),
        "llm".into(),
        move |downloaded, total| {
            let _ = tx_prog.send(DlEv::Prog(downloaded, total));
        },
        move |outcome| {
            let _ = tx_done.send(DlEv::Done(outcome.map_err(|e| e.to_string())));
        },
    );
    println!("[hf-cancel] started download id={id}");

    // Wait until genuinely mid-stream (>5 MB on disk path), then cancel.
    let t0 = Instant::now();
    let cancel_sent = loop {
        match rx.recv_timeout(Duration::from_secs(300)) {
            Ok(DlEv::Prog(downloaded, total)) => {
                if downloaded > 5 * 1024 * 1024 {
                    println!(
                        "[hf-cancel] cancelling at {} bytes (total {:?}) after {:?}",
                        downloaded,
                        total,
                        t0.elapsed()
                    );
                    break engine.cancel_hf_download(&id);
                }
            }
            Ok(DlEv::Done(_)) => {
                panic!("download finished before the cancel point; cannot test mid-stream cancel")
            }
            Err(_) => panic!("timed out waiting for download progress"),
        }
    };
    assert!(cancel_sent, "cancel must target a live download id");

    // The done outcome must be the cancelled marker (not registered, not failed).
    let outcome = loop {
        match rx.recv_timeout(Duration::from_secs(120)) {
            Ok(DlEv::Done(o)) => break o,
            Ok(DlEv::Prog(downloaded, _)) => {
                println!("[hf-cancel] in-flight progress after cancel: {downloaded} bytes");
            }
            Err(_) => panic!("timed out waiting for cancel to take effect"),
        }
    };
    match &outcome {
        Ok(None) => println!("[hf-cancel] done outcome: cancelled (Ok(None))"),
        other => panic!("expected cancelled Ok(None), got {other:?}"),
    }

    assert!(!part.exists(), "partial .part file must be deleted on cancel");
    assert!(!dest.exists(), "final file must never be created on cancel");
    assert!(
        !engine
            .list_models()
            .iter()
            .any(|m| m.id == "tinyllama-1.1b-chat-v1.0.Q2_K"),
        "cancelled download must not appear in list_models"
    );
    println!("[hf-cancel] partial gone, registry clean, button-state equivalent: reset");
}
