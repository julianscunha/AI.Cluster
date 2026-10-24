"""Gera tests/fixtures/smollm2-reference.json: ids do tokenizador e logits do último token.

Uso (a partir de code/): docker run --rm -v "$PWD:/w" -w /w python:3.12-slim sh -c \
  "pip install -q torch transformers --extra-index-url https://download.pytorch.org/whl/cpu && \
   python crates/engine/reference/smollm2_reference.py models/gpt2 crates/engine/tests/fixtures/smollm2-reference.json"
"""
import json
import sys

import torch
from transformers import AutoModelForCausalLM, AutoTokenizer

model_dir, out = sys.argv[1], sys.argv[2]
tok = AutoTokenizer.from_pretrained(model_dir)
model = AutoModelForCausalLM.from_pretrained(model_dir, torch_dtype=torch.float32).eval()

TOKENIZER_TEXTS = [
    "Hello world",
    "don't stop, we're 100% sure!",
    "  leading and trailing  ",
    "line one\nline two\n\n  indented",
    "naïve café – 数字 123",
    "In 2024, pi is 3.14159 and 100% of 7 is 7.",
    "The quick brown fox jumps over the lazy dog.",
]
LOGIT_TEXTS = [
    "The capital of France is",
    "Once upon a time",
    "def add(a, b):\n    return",
]

result = {"tokenizer": [], "logits": []}
for text in TOKENIZER_TEXTS:
    result["tokenizer"].append({"text": text, "ids": tok.encode(text)})
for text in LOGIT_TEXTS:
    ids = tok.encode(text)
    with torch.no_grad():
        logits = model(torch.tensor([ids])).logits[0, -1]
    top = torch.topk(logits, 10)
    result["logits"].append(
        {
            "text": text,
            "ids": ids,
            "top": [[int(i), float(v)] for v, i in zip(top.values, top.indices)],
        }
    )
json.dump(result, open(out, "w"), ensure_ascii=False, indent=1)
print("ok", out)
