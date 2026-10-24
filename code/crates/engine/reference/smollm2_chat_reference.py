"""Gera tests/fixtures/smollm2-chat-reference.json: ids do template ChatML e a continuação gulosa.

Uso (a partir de code/): docker run --rm -v "$PWD:/w" -w /w python:3.12-slim sh -c \
  "pip install -q torch transformers --extra-index-url https://download.pytorch.org/whl/cpu && \
   python crates/engine/reference/smollm2_chat_reference.py models/smollm2-instruct crates/engine/tests/fixtures/smollm2-chat-reference.json"
"""
import json
import sys

import torch
from transformers import AutoModelForCausalLM, AutoTokenizer

model_dir, out = sys.argv[1], sys.argv[2]
tok = AutoTokenizer.from_pretrained(model_dir)
model = AutoModelForCausalLM.from_pretrained(model_dir, torch_dtype=torch.float32).eval()

CONVERSATIONS = [
    [{"role": "user", "content": "What is the capital of France?"}],
    [
        {"role": "system", "content": "You answer in one short sentence."},
        {"role": "user", "content": "Name a primary color."},
    ],
    [
        {"role": "user", "content": "Hi!"},
        {"role": "assistant", "content": "Hello, how can I help?"},
        {"role": "user", "content": "Say 12 apples."},
    ],
]

cases = []
for messages in CONVERSATIONS:
    ids = tok.apply_chat_template(messages, add_generation_prompt=True, tokenize=True)
    if isinstance(ids, dict) or hasattr(ids, "input_ids"):
        ids = ids["input_ids"]
    with torch.no_grad():
        generated = model.generate(
            torch.tensor([ids]),
            max_new_tokens=10,
            do_sample=False,
            pad_token_id=tok.pad_token_id,
        )[0][len(ids):].tolist()
    cases.append({"messages": messages, "ids": ids, "generated": generated, "text": tok.decode(generated)})

json.dump(cases, open(out, "w", encoding="utf-8"), ensure_ascii=False, indent=1)
print("ok", [c["text"] for c in cases])
