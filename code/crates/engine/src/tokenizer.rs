//! GPT-2 byte-level BPE: `vocab.json` (token -> id) and `merges.txt` (ranked pairs).
//!
//! The pre-tokenizer is the GPT-2 regex
//! `'s|'t|'re|'ve|'m|'ll|'d| ?\p{L}+| ?\p{N}+| ?[^\s\p{L}\p{N}]+|\s+(?!\S)|\s+`
//! written by hand, because it needs a lookahead the `regex` crate does not offer.
//! `char::is_alphabetic`/`is_numeric` stand in for `\p{L}`/`\p{N}`; the parity fixture covers
//! accents, CJK and digits. shortcut: `is_alphabetic` also counts combining marks of some Indic
//! scripts that `\p{L}` does not, so ids can differ there; swap in a general-category table if
//! such scripts matter.

use std::collections::HashMap;
use std::path::Path;

/// Longest text `encode` accepts; far above the 1024-token context of GPT-2.
const MAX_TEXT_BYTES: usize = 64 * 1024;

pub struct Tokenizer {
    vocab: HashMap<String, u32>,
    tokens: Vec<String>,
    ranks: HashMap<(String, String), usize>,
    byte_to_char: [char; 256],
    char_to_byte: HashMap<char, u8>,
}

/// The reversible byte -> printable char table GPT-2 uses, so every byte has a visible stand-in.
fn byte_table() -> [char; 256] {
    let mut table = ['\0'; 256];
    let mut extra = 0u32;
    for byte in 0..=255u32 {
        let printable = (33..=126).contains(&byte)
            || (161..=172).contains(&byte)
            || (174..=255).contains(&byte);
        table[byte as usize] = if printable {
            char::from_u32(byte).unwrap()
        } else {
            let stand_in = char::from_u32(256 + extra).unwrap();
            extra += 1;
            stand_in
        };
    }
    table
}

#[derive(PartialEq, Clone, Copy)]
enum Class {
    Letter,
    Number,
    Space,
    Other,
}

fn class(c: char) -> Class {
    if c.is_alphabetic() {
        Class::Letter
    } else if c.is_numeric() {
        Class::Number
    } else if c.is_whitespace() {
        Class::Space
    } else {
        Class::Other
    }
}

/// Splits `text` the way the GPT-2 regex does.
pub fn pre_tokenize(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut pieces = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let contraction = if chars[i] == '\'' {
            contraction(&chars[i + 1..])
        } else {
            None
        };
        let end = if let Some(len) = contraction {
            i + 1 + len
        } else {
            // An optional single space belongs to the run that follows it.
            let lead = usize::from(
                chars[i] == ' ' && i + 1 < chars.len() && class(chars[i + 1]) != Class::Space,
            );
            let kind = class(chars[i + lead]);
            let mut j = i + lead;
            while j < chars.len() && class(chars[j]) == kind {
                j += 1;
            }
            // `\s+(?!\S)`: when a word follows a run of spaces, the last one goes with it.
            if kind == Class::Space && j < chars.len() && j - i > 1 {
                j -= 1;
            }
            j
        };
        pieces.push(chars[i..end].iter().collect());
        i = end;
    }
    pieces
}

/// Length of the contraction suffix (`s t m d re ve ll`) at the start of `rest`, if any.
fn contraction(rest: &[char]) -> Option<usize> {
    match rest {
        ['s' | 't' | 'm' | 'd', ..] => Some(1),
        ['r', 'e', ..] | ['v', 'e', ..] | ['l', 'l', ..] => Some(2),
        _ => None,
    }
}

impl Tokenizer {
    pub fn open(dir: &Path) -> Result<Self, String> {
        let read = |name: &str| {
            std::fs::read_to_string(dir.join(name)).map_err(|err| format!("{name}: {err}"))
        };
        Self::new(&read("vocab.json")?, &read("merges.txt")?)
    }

    pub fn new(vocab_json: &str, merges: &str) -> Result<Self, String> {
        let vocab: HashMap<String, u32> =
            serde_json::from_str(vocab_json).map_err(|err| format!("vocab.json: {err}"))?;
        let mut tokens = vec![String::new(); vocab.len()];
        for (token, id) in &vocab {
            *tokens
                .get_mut(*id as usize)
                .ok_or("vocab.json: ids não são densos")? = token.clone();
        }
        let ranks = merges
            .lines()
            .filter(|line| !line.starts_with("#version") && !line.is_empty())
            .enumerate()
            .map(|(rank, line)| {
                let (left, right) = line.split_once(' ').ok_or("merges.txt: linha inválida")?;
                Ok(((left.to_string(), right.to_string()), rank))
            })
            .collect::<Result<_, &str>>()?;
        let byte_to_char = byte_table();
        let char_to_byte = byte_to_char
            .iter()
            .enumerate()
            .map(|(b, c)| (*c, b as u8))
            .collect();
        Ok(Self {
            vocab,
            tokens,
            ranks,
            byte_to_char,
            char_to_byte,
        })
    }

    pub fn vocab_size(&self) -> usize {
        self.tokens.len()
    }

    pub fn encode(&self, text: &str) -> Result<Vec<u32>, String> {
        // The merge loop is quadratic in the length of a space-free run.
        if text.len() > MAX_TEXT_BYTES {
            return Err(format!("texto acima de {MAX_TEXT_BYTES} bytes"));
        }
        let mut ids = Vec::new();
        for piece in pre_tokenize(text) {
            let symbols = piece
                .bytes()
                .map(|b| self.byte_to_char[b as usize].to_string())
                .collect();
            for symbol in self.merge(symbols) {
                let id = self
                    .vocab
                    .get(&symbol)
                    .ok_or_else(|| format!("símbolo fora do vocabulário: {symbol:?}"))?;
                ids.push(*id);
            }
        }
        Ok(ids)
    }

    /// Repeatedly merges the lowest-ranked adjacent pair until none is in `merges.txt`.
    fn merge(&self, mut symbols: Vec<String>) -> Vec<String> {
        loop {
            let best = (0..symbols.len().saturating_sub(1))
                .filter_map(|i| {
                    let pair = (symbols[i].clone(), symbols[i + 1].clone());
                    self.ranks.get(&pair).map(|rank| (*rank, i))
                })
                .min();
            let Some((_, at)) = best else { return symbols };
            let right = symbols.remove(at + 1);
            symbols[at].push_str(&right);
        }
    }

    pub fn decode(&self, ids: &[u32]) -> Result<String, String> {
        let mut bytes = Vec::new();
        for id in ids {
            let token = self
                .tokens
                .get(*id as usize)
                .ok_or_else(|| format!("id fora do vocabulário: {id}"))?;
            for c in token.chars() {
                bytes.push(
                    *self
                        .char_to_byte
                        .get(&c)
                        .ok_or("token fora da tabela de bytes")?,
                );
            }
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_like_the_gpt2_regex() {
        assert_eq!(pre_tokenize("Hello world"), ["Hello", " world"]);
        assert_eq!(pre_tokenize("don't stop"), ["don", "'t", " stop"]);
        assert_eq!(pre_tokenize("we're"), ["we", "'re"]);
        assert_eq!(pre_tokenize("a  b"), ["a", " ", " b"]);
        assert_eq!(pre_tokenize("trailing  "), ["trailing", "  "]);
        assert_eq!(pre_tokenize("100%!"), ["100", "%!"]);
        assert_eq!(pre_tokenize("x\n\n  y"), ["x", "\n\n ", " y"]);
        assert_eq!(pre_tokenize("'x"), ["'", "x"]);
        assert_eq!(pre_tokenize(" 12 ab"), [" 12", " ab"]);
        assert!(pre_tokenize("").is_empty());
    }

    #[test]
    fn bytes_round_trip_through_the_table() {
        let table = byte_table();
        let distinct: std::collections::HashSet<_> = table.iter().collect();
        assert_eq!(distinct.len(), 256);
        assert_eq!(table[b'A' as usize], 'A');
        assert_eq!(table[b' ' as usize], 'Ġ');
    }

    #[test]
    fn merges_by_rank_and_decodes() {
        // Tiny vocabulary: the bytes of "ab " plus the merge a+b.
        let vocab = r#"{"a":0,"b":1,"Ġ":2,"ab":3}"#;
        let tokenizer = Tokenizer::new(vocab, "#version: 0.2\na b\n").unwrap();
        assert_eq!(tokenizer.encode("ab ").unwrap(), vec![3, 2]);
        assert_eq!(tokenizer.encode("ba").unwrap(), vec![1, 0]);
        assert_eq!(tokenizer.decode(&[3, 2]).unwrap(), "ab ");
        assert!(tokenizer.encode("z").is_err());
        assert!(tokenizer.decode(&[99]).is_err());
    }
}
