// Build script for the `acp` crate.
//
// When the `embedding` feature is enabled, the desktop app loads an INT8
// quantized ONNX model at startup. To keep `cargo build` reproducible and
// air-gap friendly, this script does **not** download the model
// automatically. Instead it supports three modes, checked in order:
//
//   1. The model already exists at `$OUT_DIR/all-MiniLM-L6-v2-INT8.onnx`
//      and is non-empty. Use it as-is.
//   2. `SHOVEL_EMBEDDING_MODEL_PATH` points to a file. Copy it into
//      `$OUT_DIR`. This is the recommended path for CI and air-gapped
//      builds: pre-stage the model in the build environment.
//   3. `SHOVEL_EMBEDDING_AUTO_DOWNLOAD=1` is set. Download from HuggingFace
//      using `reqwest` (no external `curl` binary required) and verify the
//      downloaded payload is non-empty. Off by default so a fresh clone
//      fails fast with a clear message instead of silently waiting on
//      the network.
//
// We do not embed a SHA256 of the model here. Hashing the model would
// require shipping the digest in this file, which means a model re-upload
// on HuggingFace would break every historical build. Network integrity is
// left to TLS, and a size sanity check rejects truncated responses.

#[cfg(feature = "embedding")]
mod embedding {
    use std::path::{Path, PathBuf};

    const MODEL_URL: &str = "https://huggingface.co/Ayeshas21/sentence-transformers-all-MiniLM-L6-v2-quantized/resolve/main/model-quant.onnx";
    const MODEL_FILENAME: &str = "all-MiniLM-L6-v2-INT8.onnx";
    const MIN_MODEL_SIZE: u64 = 1024 * 1024; // 1 MiB — refuse anything smaller.

    pub(super) fn ensure_model(out_dir: &Path) -> Result<(), String> {
        let model_path = out_dir.join(MODEL_FILENAME);

        if is_valid_existing(&model_path) {
            println!(
                "cargo:warning=Reusing existing embedding model at {}",
                model_path.display()
            );
            return Ok(());
        }

        if let Some(source) = std::env::var_os("SHOVEL_EMBEDDING_MODEL_PATH") {
            let source = PathBuf::from(source);
            return copy_from_path(&source, &model_path);
        }

        if env_flag("SHOVEL_EMBEDDING_AUTO_DOWNLOAD") {
            return download_from_url(MODEL_URL, &model_path);
        }

        Err(format!(
            "Embedding model is required for the `embedding` feature but is not available.\n\
             Provide the model in one of these ways:\n\
             \n\
             1. Pre-stage it at $OUT_DIR/{MODEL_FILENAME} (any non-empty file is accepted).\n\
             2. Set SHOVEL_EMBEDDING_MODEL_PATH=/path/to/{MODEL_FILENAME} in the build environment.\n\
             3. Set SHOVEL_EMBEDDING_AUTO_DOWNLOAD=1 to fetch it from {MODEL_URL}.\n\
             \n\
             Option 3 should not be used in CI or air-gapped builds."
        ))
    }

    fn is_valid_existing(path: &Path) -> bool {
        match std::fs::metadata(path) {
            Ok(meta) if meta.len() >= MIN_MODEL_SIZE => true,
            Ok(_) => {
                println!(
                    "cargo:warning=Ignoring empty or too-small model at {}",
                    path.display()
                );
                false
            }
            Err(_) => false,
        }
    }

    fn copy_from_path(source: &Path, dest: &Path) -> Result<(), String> {
        let meta = std::fs::metadata(source).map_err(|err| {
            format!(
                "SHOVEL_EMBEDDING_MODEL_PATH points to {} but it could not be read: {err}",
                source.display()
            )
        })?;
        if meta.len() < MIN_MODEL_SIZE {
            return Err(format!(
                "Model at {} is {} bytes, expected at least {MIN_MODEL_SIZE}. \
                 Refusing to use a truncated file.",
                source.display(),
                meta.len()
            ));
        }
        std::fs::copy(source, dest).map_err(|err| {
            format!(
                "Failed to copy model from {} to {}: {err}",
                source.display(),
                dest.display()
            )
        })?;
        println!(
            "cargo:warning=Copied embedding model from {} to {}",
            source.display(),
            dest.display()
        );
        Ok(())
    }

    fn download_from_url(url: &str, dest: &Path) -> Result<(), String> {
        println!(
            "cargo:warning=Downloading embedding model from {url} (SHOVEL_EMBEDDING_AUTO_DOWNLOAD=1)"
        );
        let body = reqwest::blocking::get(url)
            .and_then(|response| response.error_for_status())
            .and_then(|response| response.bytes())
            .map_err(|err| format!("Failed to download embedding model from {url}: {err}"))?;

        if (body.len() as u64) < MIN_MODEL_SIZE {
            return Err(format!(
                "Downloaded embedding model is {} bytes, expected at least {MIN_MODEL_SIZE}. \
                 The download was likely truncated; check your network and try again.",
                body.len()
            ));
        }

        std::fs::write(dest, &body).map_err(|err| {
            format!(
                "Failed to write embedding model to {}: {err}",
                dest.display()
            )
        })?;
        println!(
            "cargo:warning=Downloaded {} bytes to {}",
            body.len(),
            dest.display()
        );
        Ok(())
    }

    fn env_flag(name: &str) -> bool {
        matches!(
            std::env::var(name)
                .ok()
                .map(|value| value.trim().to_ascii_lowercase()),
            Some(value) if matches!(value.as_str(), "1" | "true" | "yes" | "on")
        )
    }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=SHOVEL_EMBEDDING_MODEL_PATH");
    println!("cargo:rerun-if-env-changed=SHOVEL_EMBEDDING_AUTO_DOWNLOAD");

    #[cfg(feature = "embedding")]
    {
        use std::path::PathBuf;

        let out_dir = match std::env::var("OUT_DIR") {
            Ok(dir) => PathBuf::from(dir),
            Err(_) => {
                eprintln!("build script error: OUT_DIR is not set");
                std::process::exit(1);
            }
        };

        if let Err(err) = embedding::ensure_model(&out_dir) {
            eprintln!("Embedding model setup failed:\n{err}");
            std::process::exit(1);
        }
    }
}
