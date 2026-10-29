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
/// Longest run of bytes without a break the pre-tokenizer would keep together. The merge loop
/// is quadratic in it, so a long unbroken run is refused instead of tokenized.
const MAX_PIECE_BYTES: usize = 512;

pub struct Tokenizer {
    vocab: HashMap<String, u32>,
    tokens: Vec<String>,
    /// `"left\0right"` -> rank, so a lookup needs no allocation (a byte symbol is never `\0`).
    ranks: HashMap<String, usize>,
    byte_to_char: [char; 256],
    char_to_byte: HashMap<char, u8>,
    /// Every digit is its own piece before the regex runs (SmolLM2's `Digits` pre-tokenizer).
    split_digits: bool,
    /// The Qwen2 pre-tokenizer regex instead of GPT-2's (`tokenizer.json` says which).
    qwen_style: bool,
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
    pre_tokenize_with(text, false)
}

/// The GPT-2 split, optionally after cutting out every digit as its own piece, as the HF
/// `Digits(individual_digits)` pre-tokenizer does before the byte-level regex.
pub fn pre_tokenize_with(text: &str, split_digits: bool) -> Vec<String> {
    if !split_digits {
        return split_regex(text);
    }
    let mut pieces = Vec::new();
    let mut run = String::new();
    for c in text.chars() {
        if c.is_numeric() {
            pieces.extend(split_regex(&run));
            run.clear();
            pieces.push(c.to_string());
        } else {
            run.push(c);
        }
    }
    pieces.extend(split_regex(&run));
    pieces
}

fn split_regex(text: &str) -> Vec<String> {
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

/// Splits `text` the way the Qwen2 regex does:
/// `(?i:'s|'t|'re|'ve|'m|'ll|'d)|[^\r\n\p{L}\p{N}]?\p{L}+|\p{N}| ?[^\s\p{L}\p{N}]+[\r\n]*|\s*[\r\n]+|\s+(?!\S)|\s+`.
/// Alternatives are tried in that order at each position, like the regex engine does.
pub fn split_qwen(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let is_newline = |c: char| c == '\r' || c == '\n';
    let letter = |c: char| class(c) == Class::Letter;
    let other = |c: char| class(c) == Class::Other;
    let mut pieces = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let end = 'piece: {
            // (?i:'s|'t|'re|'ve|'m|'ll|'d)
            if c == '\'' {
                let rest: Vec<char> = chars[i + 1..]
                    .iter()
                    .map(|c| c.to_ascii_lowercase())
                    .collect();
                if let Some(len) = contraction(&rest) {
                    break 'piece i + 1 + len;
                }
            }
            // [^\r\n\p{L}\p{N}]?\p{L}+
            let lead = usize::from(!letter(c) && !is_newline(c) && class(c) != Class::Number);
            if chars.get(i + lead).is_some_and(|c| letter(*c)) {
                let mut j = i + lead;
                while j < chars.len() && letter(chars[j]) {
                    j += 1;
                }
                break 'piece j;
            }
            // \p{N}
            if class(c) == Class::Number {
                break 'piece i + 1;
            }
            // ` ?[^\s\p{L}\p{N}]+[\r\n]*`
            let start = i + usize::from(c == ' ');
            if chars.get(start).is_some_and(|c| other(*c)) {
                let mut j = start;
                while j < chars.len() && other(chars[j]) {
                    j += 1;
                }
                while j < chars.len() && is_newline(chars[j]) {
                    j += 1;
                }
                break 'piece j;
            }
            // Whitespace: \s*[\r\n]+, then \s+(?!\S), then \s+.
            let mut run = i;
            while run < chars.len() && class(chars[run]) == Class::Space {
                run += 1;
            }
            if let Some(last) = (i..run).rev().find(|k| is_newline(chars[*k])) {
                break 'piece last + 1;
            }
            if run < chars.len() && run - i > 1 {
                break 'piece run - 1;
            }
            run
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

/// `(dígitos soltos, regex do Qwen2)` segundo o `pre_tokenizer` de um `tokenizer.json`.
fn pre_tokenizer_style(json: &serde_json::Value) -> (bool, bool) {
    let text = json["pre_tokenizer"].to_string();
    (
        text.contains("\"individual_digits\":true"),
        text.contains(r"\\p{L}+|\\p{N}|"),
    )
}

impl Tokenizer {
    pub fn open(dir: &Path) -> Result<Self, String> {
        let read = |name: &str| {
            std::fs::read_to_string(dir.join(name)).map_err(|err| format!("{name}: {err}"))
        };
        let mut tokenizer = Self::new(&read("vocab.json")?, &read("merges.txt")?)?;
        // Only the pre-tokenizer setting is read from tokenizer.json; the vocabulary and merges
        // come from the two files above, which hold the same data.
        if let Ok(text) = read("tokenizer.json") {
            let json: serde_json::Value =
                serde_json::from_str(&text).map_err(|err| format!("tokenizer.json: {err}"))?;
            (tokenizer.split_digits, tokenizer.qwen_style) = pre_tokenizer_style(&json);
        }
        Ok(tokenizer)
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
                Ok((format!("{left}\0{right}"), rank))
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
            split_digits: false,
            qwen_style: false,
        })
    }

    pub fn vocab_size(&self) -> usize {
        self.tokens.len()
    }

    /// The id of a token written out in full, such as a special token (`<|im_end|>`). Text given
    /// to [`encode`](Self::encode) never produces one: special tokens enter a prompt only
    /// through the template, so a user cannot forge a turn boundary.
    pub fn token_id(&self, token: &str) -> Option<u32> {
        self.vocab.get(token).copied()
    }

    pub fn encode(&self, text: &str) -> Result<Vec<u32>, String> {
        if text.len() > MAX_TEXT_BYTES {
            return Err(format!("texto acima de {MAX_TEXT_BYTES} bytes"));
        }
        let mut ids = Vec::new();
        let pieces = if self.qwen_style {
            split_qwen(text)
        } else {
            pre_tokenize_with(text, self.split_digits)
        };
        for piece in pieces {
            if piece.len() > MAX_PIECE_BYTES {
                return Err(format!(
                    "trecho sem separador acima de {MAX_PIECE_BYTES} bytes"
                ));
            }
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
        let mut key = String::new();
        loop {
            let best = (0..symbols.len().saturating_sub(1))
                .filter_map(|i| {
                    key.clear();
                    key.push_str(&symbols[i]);
                    key.push('\0');
                    key.push_str(&symbols[i + 1]);
                    self.ranks.get(&key).map(|rank| (*rank, i))
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
    fn the_qwen2_regex_keeps_a_leading_mark_with_the_word_and_cuts_digits() {
        let pieces = |t: &str| split_qwen(t);
        assert_eq!(
            pieces("def add(a, b):"),
            ["def", " add", "(a", ",", " b", "):"]
        );
        assert_eq!(pieces("x 12"), ["x", " ", "1", "2"]);
        assert_eq!(pieces("I'M ok"), ["I", "'M", " ok"]);
        assert_eq!(pieces("a\n  b"), ["a", "\n", " ", " b"]);
    }

    #[test]
    fn the_qwen2_regex_edge_cases() {
        let cut = |t: &str| split_qwen(t);
        assert_eq!(cut(""), Vec::<String>::new());
        assert_eq!(
            cut("it's THEY'RE we'll"),
            ["it", "'s", " THEY", "'RE", " we", "'ll"]
        );
        assert_eq!(cut("a\r\nb"), ["a", "\r\n", "b"]);
        assert_eq!(cut("!\nx"), ["!\n", "x"]);
        assert_eq!(cut("a  "), ["a", "  "]);
        assert_eq!(cut("x  y"), ["x", " ", " y"]);
        assert_eq!(cut("a\tb"), ["a", "\tb"]);
        assert_eq!(cut("é中文"), ["é中文"]);
        assert_eq!(cut("٣1"), ["٣", "1"]);
        // Nada se perde: juntar os pedaços devolve o texto.
        let text = "def f(x):\r\n\treturn x  +  1\n\n# fim";
        assert_eq!(cut(text).concat(), text);
    }

    #[test]
    fn only_a_qwen_tokenizer_json_turns_the_qwen_regex_on() {
        let gpt2 =
            serde_json::json!({"pre_tokenizer": {"type": "ByteLevel", "add_prefix_space": false}});
        assert_eq!(pre_tokenizer_style(&gpt2), (false, false));
        let digits =
            serde_json::json!({"pre_tokenizer": {"type": "Digits", "individual_digits": true}});
        assert_eq!(pre_tokenizer_style(&digits), (true, false));
        let qwen = serde_json::json!({"pre_tokenizer": {"type": "Split", "pattern": {"Regex":
            r"(?i:'s|'t)|[^\r\n\p{L}\p{N}]?\p{L}+|\p{N}| ?[^\s\p{L}\p{N}]+[\r\n]*"}}});
        assert_eq!(pre_tokenizer_style(&qwen), (false, true));
        assert_eq!(pre_tokenizer_style(&serde_json::json!({})), (false, false));
    }

    #[test]
    fn individual_digits_are_cut_out_before_the_regex() {
        assert_eq!(
            pre_tokenize_with("a 12 b", true),
            ["a", " ", "1", "2", " b"]
        );
        assert_eq!(pre_tokenize_with("3.14", true), ["3", ".", "1", "4"]);
        assert_eq!(pre_tokenize_with("abc", true), pre_tokenize("abc"));
        assert_eq!(pre_tokenize_with("a 12", false), ["a", " 12"]);
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
        assert!(tokenizer.encode(&"a".repeat(MAX_PIECE_BYTES + 1)).is_err());
        assert!(tokenizer.decode(&[99]).is_err());
    }
}
