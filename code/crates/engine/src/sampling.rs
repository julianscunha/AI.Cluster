//! Como o próximo token é escolhido a partir dos logits: guloso (padrão, determinístico) ou
//! amostrado com temperatura e núcleo (top-p).

/// Parâmetros de amostragem de uma geração. `temperature == 0` é guloso e ignora o resto, então o
/// mesmo pedido dá sempre a mesma resposta (e pode ser servido do cache).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sampling {
    /// 0 = guloso; acima de 0, divide os logits antes do softmax (1 = distribuição do modelo).
    pub temperature: f64,
    /// Fração de massa de probabilidade mantida, de maior para menor; 1 mantém tudo.
    pub top_p: f64,
    /// Semente do sorteio; a mesma semente reproduz a mesma geração.
    pub seed: u64,
}

impl Sampling {
    pub const GREEDY: Sampling = Sampling {
        temperature: 0.0,
        top_p: 1.0,
        seed: 0,
    };

    pub fn is_greedy(&self) -> bool {
        self.temperature <= 0.0
    }
}

/// Gerador pseudoaleatório pequeno (splitmix64): sem dependência, e reproduzível pela semente.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Um número em [0, 1).
    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// O índice do maior logit (empate: o último, como o `max_by` de sempre).
fn argmax(logits: &[f64]) -> Option<u32> {
    logits
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(id, _)| id as u32)
}

/// Escolhe o próximo token. Guloso sem sorteio; senão softmax com temperatura, corte pelo núcleo
/// e sorteio entre os que ficaram. `None` só com vocabulário vazio.
pub fn pick(logits: &[f64], sampling: &Sampling, rng: &mut Rng) -> Option<u32> {
    if sampling.is_greedy() {
        return argmax(logits);
    }
    let best = logits
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(f64::NEG_INFINITY, f64::max);
    if !best.is_finite() {
        return argmax(logits);
    }
    // (token, peso) com o máximo subtraído: o expoente nunca estoura.
    let mut weighted: Vec<(u32, f64)> = logits
        .iter()
        .enumerate()
        .map(|(id, &v)| {
            let weight = if v.is_finite() {
                ((v - best) / sampling.temperature).exp()
            } else {
                0.0
            };
            (id as u32, weight)
        })
        .collect();
    let total: f64 = weighted.iter().map(|(_, w)| w).sum();
    if sampling.top_p < 1.0 {
        weighted.sort_by(|a, b| b.1.total_cmp(&a.1));
        let keep = sampling.top_p.max(f64::MIN_POSITIVE) * total;
        let mut mass = 0.0;
        let mut cut = weighted.len();
        for (index, (_, weight)) in weighted.iter().enumerate() {
            mass += weight;
            if mass >= keep {
                cut = index + 1;
                break;
            }
        }
        weighted.truncate(cut);
    }
    let kept: f64 = weighted.iter().map(|(_, w)| w).sum();
    let mut draw = rng.unit() * kept;
    for &(token, weight) in &weighted {
        if draw < weight {
            return Some(token);
        }
        draw -= weight;
    }
    weighted.last().map(|(token, _)| *token)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOGITS: [f64; 4] = [1.0, 3.0, 2.0, -1.0];

    #[test]
    fn temperatura_zero_e_o_argmax() {
        let mut rng = Rng::new(7);
        assert_eq!(pick(&LOGITS, &Sampling::GREEDY, &mut rng), Some(1));
    }

    #[test]
    fn a_mesma_semente_repete_e_outra_semente_pode_divergir() {
        let sampling = Sampling {
            temperature: 1.5,
            top_p: 1.0,
            seed: 11,
        };
        let run = |seed| {
            let mut rng = Rng::new(seed);
            (0..32)
                .map(|_| pick(&LOGITS, &sampling, &mut rng).unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(run(11), run(11));
        assert_ne!(run(11), run(12));
    }

    #[test]
    fn top_p_pequeno_deixa_so_o_mais_provavel() {
        let sampling = Sampling {
            temperature: 1.0,
            top_p: 0.1,
            seed: 0,
        };
        let mut rng = Rng::new(3);
        for _ in 0..50 {
            assert_eq!(pick(&LOGITS, &sampling, &mut rng), Some(1));
        }
    }

    #[test]
    fn temperatura_alta_espalha_e_so_sorteia_tokens_do_vocabulario() {
        let sampling = Sampling {
            temperature: 5.0,
            top_p: 1.0,
            seed: 0,
        };
        let mut rng = Rng::new(1);
        let mut seen = [false; 4];
        for _ in 0..400 {
            seen[pick(&LOGITS, &sampling, &mut rng).unwrap() as usize] = true;
        }
        assert!(seen.iter().all(|&s| s), "{seen:?}");
    }

    #[test]
    fn logits_nao_finitos_nao_quebram_o_sorteio() {
        let logits = [f64::NAN, 2.0, f64::NEG_INFINITY];
        let sampling = Sampling {
            temperature: 1.0,
            top_p: 0.9,
            seed: 0,
        };
        let mut rng = Rng::new(5);
        assert_eq!(pick(&logits, &sampling, &mut rng), Some(1));
        assert_eq!(pick(&[], &sampling, &mut rng), None);
    }
}
