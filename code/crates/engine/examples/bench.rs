//! Tempo local de um passo de geração e quanto dele é multiplicação de matrizes.
//!
//! Uso: `cargo run --release -p ai-cluster-engine --example bench -- <gpt2|smollm2> <diretório>`

use ai_cluster_common::matrix::Matrix;
use ai_cluster_engine::{LanguageModel, gpt2::Gpt2, llama::Llama, tokenizer::Tokenizer};
use std::path::Path;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (kind, dir) = (args[1].as_str(), Path::new(&args[2]));
    let tokenizer = Tokenizer::open(dir).unwrap();
    let started = Instant::now();
    let model: Box<dyn LanguageModel> = match kind {
        "gpt2" => Box::new(Gpt2::load(dir).unwrap()),
        _ => Box::new(Llama::load(dir).unwrap()),
    };
    println!("carga: {:.2?}", started.elapsed());

    let prompt = tokenizer
        .encode("The capital of France is a city that")
        .unwrap();
    let mut cache = model.new_cache();
    let started = Instant::now();
    let mut logits = model.forward(&prompt, &mut cache).unwrap();
    println!(
        "prompt de {} tokens: {:.2?}",
        prompt.len(),
        started.elapsed()
    );

    let steps = 16;
    let started = Instant::now();
    for _ in 0..steps {
        let next = logits
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0 as u32;
        logits = model.forward(&[next], &mut cache).unwrap();
    }
    let per_token = started.elapsed() / steps;
    println!("por token gerado: {per_token:.2?}");

    // Uma multiplicação 1 x 576 x 1536 (o gate/up da SmolLM2) isolada, com 1 e N threads.
    let x = Matrix::seeded(1, 576, 1);
    let w = Matrix::seeded(576, 1536, 2);
    for threads in [1, std::thread::available_parallelism().unwrap().get()] {
        let started = Instant::now();
        for _ in 0..200 {
            x.multiply_with_threads(&w, threads).unwrap();
        }
        println!(
            "matmul 1x576x1536, {threads} thread(s): {:.2?}",
            started.elapsed() / 200
        );
    }
}
