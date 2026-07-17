use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};

fn main() {
    let model = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"))
        .join("../upyr-core/assets/models/language.ngm");
    println!("cargo:rerun-if-changed={}", model.display());

    let bytes = fs::read(&model).expect("read embedded Upyr language model");
    let digest = Sha256::digest(bytes);
    let prefix = digest[..4]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    println!("cargo:rustc-env=UPYR_MODEL_SHA256_PREFIX={prefix}");
}
