//! ChatML prompts for instruct models (SmolLM2-Instruct): every turn is
//! `<|im_start|>role\ncontent<|im_end|>\n`, and the prompt ends by opening an assistant turn.
//!
//! The special tokens are put in by id and the text between them goes through the ordinary
//! encoder, so a message that contains `<|im_end|>` is just text: it cannot close its own turn
//! or open a `system` one.

use crate::tokenizer::Tokenizer;

/// Used when the conversation has no `system` message, as the model's own template does.
const DEFAULT_SYSTEM: &str = "You are a helpful AI assistant named SmolLM, trained by Hugging Face";

pub struct ChatMl {
    start: u32,
    /// Also the token that ends the assistant's reply.
    pub end: u32,
}

impl ChatMl {
    /// `None` when the vocabulary has no ChatML tokens (a base model).
    pub fn for_tokenizer(tokenizer: &Tokenizer) -> Option<Self> {
        Some(Self {
            start: tokenizer.token_id("<|im_start|>")?,
            end: tokenizer.token_id("<|im_end|>")?,
        })
    }

    /// Token ids for `messages` (role, content) followed by an open assistant turn.
    pub fn prompt(
        &self,
        tokenizer: &Tokenizer,
        messages: &[(&str, &str)],
    ) -> Result<Vec<u32>, String> {
        let mut ids = Vec::new();
        let mut turn = |role: &str, content: &str| -> Result<(), String> {
            ids.push(self.start);
            ids.extend(tokenizer.encode(&format!("{role}\n{content}"))?);
            ids.push(self.end);
            ids.extend(tokenizer.encode("\n")?);
            Ok(())
        };
        if messages.first().is_none_or(|(role, _)| *role != "system") {
            turn("system", DEFAULT_SYSTEM)?;
        }
        for (role, content) in messages {
            turn(role, content)?;
        }
        ids.push(self.start);
        ids.extend(tokenizer.encode("assistant\n")?);
        Ok(ids)
    }
}
