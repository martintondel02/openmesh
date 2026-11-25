//! Generate the OMP/1 golden vectors in `protocol/vectors/`.
//!
//! Run from anywhere in the workspace:
//!
//! ```text
//! cargo run -p openmesh-core --example gen_vectors
//! ```
//!
//! The output is deterministic; the `vectors_are_current` unit test in
//! `crates/openmesh-core/src/vectors.rs` fails if the committed JSON files drift from the
//! reference implementation.

fn main() {
    let dir = openmesh_core::vectors::default_vectors_dir();
    match openmesh_core::vectors::generate_all(&dir) {
        Ok(files) => {
            println!("wrote {} vector files to {}", files.len(), dir.display());
            for file in files {
                println!("  {file}");
            }
        }
        Err(error) => {
            eprintln!("failed to write vectors to {}: {error}", dir.display());
            std::process::exit(1);
        }
    }
}
