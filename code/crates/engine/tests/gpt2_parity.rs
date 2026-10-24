//! Paridade com o `transformers` (fixture gerada por `reference/gpt2_reference.py`).
//!
//! Precisa dos pesos (cerca de 500 MB), que não ficam no repositório. Sem
//! `AI_CLUSTER_GPT2_DIR` apontando para o diretório com `model.safetensors`, `vocab.json`,
//! `merges.txt` e `config.json`, os testes avisam e passam sem verificar nada.

use ai_cluster_engine::{LanguageModel, gpt2::Gpt2, tokenizer::Tokenizer};
use std::path::PathBuf;

fn model_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("AI_CLUSTER_GPT2_DIR").map(PathBuf::from);
    if dir.is_none() {
        eprintln!("AI_CLUSTER_GPT2_DIR não definido: teste de paridade ignorado");
    }
    dir
}

fn fixture() -> serde_json::Value {
    serde_json::from_str(include_str!("fixtures/gpt2-reference.json")).unwrap()
}

fn ids(value: &serde_json::Value) -> Vec<u32> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u32)
        .collect()
}

#[test]
fn the_tokenizer_matches_the_reference() {
    let Some(dir) = model_dir() else { return };
    let tokenizer = Tokenizer::open(&dir).unwrap();
    for case in fixture()["tokenizer"].as_array().unwrap() {
        let text = case["text"].as_str().unwrap();
        let expected = ids(&case["ids"]);
        assert_eq!(tokenizer.encode(text).unwrap(), expected, "texto: {text:?}");
        assert_eq!(
            tokenizer.decode(&expected).unwrap(),
            text,
            "ida e volta: {text:?}"
        );
    }
}

#[test]
fn the_logits_match_the_reference() {
    let Some(dir) = model_dir() else { return };
    let model = Gpt2::load(&dir).unwrap();
    let tokenizer = Tokenizer::open(&dir).unwrap();
    for case in fixture()["logits"].as_array().unwrap() {
        let text = case["text"].as_str().unwrap();
        let prompt = tokenizer.encode(text).unwrap();
        assert_eq!(prompt, ids(&case["ids"]), "texto: {text:?}");
        let logits = model.next_token_logits(&prompt).unwrap();
        assert_eq!(logits.len(), tokenizer.vocab_size());

        let top = case["top"].as_array().unwrap();
        let best = top[0][0].as_u64().unwrap() as usize;
        let ours = logits
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0;
        assert_eq!(
            ours, best,
            "o próximo token mais provável difere em {text:?}"
        );
        for entry in top {
            let (id, expected) = (
                entry[0].as_u64().unwrap() as usize,
                entry[1].as_f64().unwrap(),
            );
            assert!(
                (logits[id] - expected).abs() < 1e-3,
                "{text:?}: logit do token {id} = {}, esperado {expected}",
                logits[id]
            );
        }
    }
}

#[test]
fn cached_generation_matches_recomputing_the_whole_prompt() {
    let Some(dir) = model_dir() else { return };
    let model = Gpt2::load(&dir).unwrap();
    let tokenizer = Tokenizer::open(&dir).unwrap();
    let prompt = tokenizer.encode("The capital of France is").unwrap();
    let no_cancel = std::sync::atomic::AtomicBool::new(false);
    let generated = model.generate(&prompt, 6, None, &no_cancel).unwrap();
    assert_eq!(generated.len(), 6);

    // Without a cache every step re-runs the growing sequence from scratch.
    let mut context = prompt.clone();
    for expected in &generated {
        let logits = model.next_token_logits(&context).unwrap();
        let best = logits
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0;
        assert_eq!(best as u32, *expected);
        context.push(best as u32);
    }
    // The tokens decode to text, and a stop token ends the run early (and is kept).
    assert!(!tokenizer.decode(&generated).unwrap().is_empty());
    let stopped = model
        .generate(&prompt, 6, Some(generated[2]), &no_cancel)
        .unwrap();
    assert!(stopped.len() <= 3);
    assert_eq!(stopped, generated[..stopped.len()]);
}

#[test]
fn a_cancelled_generation_stops() {
    let Some(dir) = model_dir() else { return };
    let model = Gpt2::load(&dir).unwrap();
    let cancelled = std::sync::atomic::AtomicBool::new(true);
    assert!(model.generate(&[15496], 4, None, &cancelled).is_err());
}
