//! Inference Engine (Fase J2): the math of a transformer, kept apart from placement and
//! scheduling. It runs on the `Matrix` the cluster's workers already multiply, so a later slice
//! can send those products through the execution graph without touching the model code.

pub mod gpt2;
pub mod safetensors;
pub mod tokenizer;
