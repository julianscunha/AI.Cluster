---
title: "F12 — Manifest de modelo"
aliases: ["N1", "N2", "manifest.json", "versão dos pesos"]
tags: [implementacao/fase/12, implementacao/fatia, implementacao/estado/parcial]
type: implementacao/fatia
status: parcial
superado: false
fase: "[[fase-12-model-runtime]]"
codigos-antigos: ["N1", "N2"]
sistema: ["[[model-manager]]", "[[inference-engine]]"]
evidencia: ["code/tests/phase-n-manifest.sh"]
---
# F12 — Manifest de modelo

- [ ] Superado

## Objetivo

Cada worker lia os pesos do próprio disco sem ninguém conferir que eram os mesmos do gateway, e a API não dizia qual versão respondia. Esta fatia dá a um diretório de modelo um `manifest.json` com nome, versão e o SHA-256 de cada arquivo, e faz o gateway e os workers recusarem pesos que não conferem.

## O que foi feito

- `engine::manifest` (`crates/engine/src/manifest.rs`): `Manifest::build` lista e faz o hash (em blocos de 1 MiB) dos arquivos do diretório, em ordem de nome, então os mesmos pesos dão o mesmo manifest; `verify` confere tamanho e hash de cada arquivo; `from_json` só aceita nomes de arquivo simples (letras, dígitos e `._-`), sem repetição, com `sha256` de 64 hexadecimais, no máximo 1024 arquivos, `name` e `version` simples (seguros em cabeçalho HTTP), e exige que `model.safetensors` esteja listado; o `manifest.json` não passa de 1 MiB.
- CLI local: `ai-cluster model manifest <dir> --name N --version V` grava o manifest; `ai-cluster model verify <dir>` confere.
- Gateway: no start, cada diretório de modelo com manifest é verificado e o processo **não sobe** se não conferir (a mensagem nomeia o arquivo). `AI_CLUSTER_REQUIRE_MANIFEST=1` recusa também um modelo sem manifest. A versão sai em `GET /v1/models` (`version`) e no cabeçalho `x-model-version` do chat (inteiro e SSE).
- Worker: antes de abrir o canal de fatias, verifica o manifest de cada subdiretório de `AI_CLUSTER_MODEL_DIR` que tenha um; se algum não conferir, o worker para. Com `AI_CLUSTER_REQUIRE_MANIFEST=1` o worker também recusa um subdiretório sem manifest.

## Evidência

`bash code/tests/phase-n-manifest.sh` (precisa de `code/models/gpt2`): A) gera e confere o manifest; B) o gateway sobe e mostra `124m-test-1` em `/v1/models` e em `x-model-version`; C) um manifest adulterado (feito com links para os mesmos pesos) impede o gateway de subir e nomeia `config.json`; D) com `AI_CLUSTER_REQUIRE_MANIFEST=1` um diretório sem manifest é recusado. Unitários: round trip e detecção de um byte trocado, manifest ausente, caminho fora do diretório e um hash conhecido (SHA-256 de `abc`).

## Limites

> [!warning] Confere no start, não a cada leitura
> A checagem acontece quando o processo sobe; entre o hash e o carregamento há uma janela em que o arquivo pode ser trocado. Symlinks dentro do diretório são seguidos (modelos costumam ser linkados), então quem controla o diretório controla o alvo. Um arquivo trocado depois do start não é pego, e o worker lê as fatias do disco sob demanda. O manifest também não é assinado: quem escreve no diretório do modelo escreve no manifest, então ele detecta corrupção e divergência, não adulteração por quem tem acesso ao disco. Não há registry central: o manifest vive ao lado dos pesos, e gateway e workers não comparam as versões entre si.

> [!todo] O resto do registry
> Variantes e precisão (N3), um registry consultável pelo controller, comparação de versão entre gateway e workers, e hash por fatia carregada. Ver [[fase-12-model-runtime]].

## Próximos passos

- O worker anunciar as versões que tem e o controller recusar fatias de versão diferente da do gateway.
- Manifest assinado.
