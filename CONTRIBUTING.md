# Contribuindo

Obrigado pelo interesse. Idioma do repositório (código comentado, docs, commits, issues): **pt-br**.

## Antes de começar

1. Leia [`AGENTS.md`](AGENTS.md) (layout dos crates, convenções, armadilhas) e [`docs/implementation/pendencias.md`](docs/implementation/pendencias.md) (o que falta).
2. Para mudança grande, abra uma issue antes descrevendo o problema e a abordagem.

## Fluxo

1. Fork e branch a partir de `main` (`feat/…`, `fix/…`, `docs/…`).
2. Rode **tudo** em `code/` antes de abrir o PR (o CI faz o mesmo):
   ```bash
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   cargo test --workspace --all-features
   ```
3. Mudança de comportamento leva teste. Mudança em rede, boot ou imagem deve ser validada em hardware quando aplicável e a evidência vai na descrição do PR.
4. **Docs são parte do código:** atualize `docs/system/` e `docs/guia/` e ajuste `docs/implementation/pendencias.md` se um item fechou; registre um ADR se a decisão for arquitetural (formato em [`docs/convencoes.md`](docs/convencoes.md)).
5. Mensagens de commit no formato `tipo(escopo): resumo` (`feat`, `fix`, `docs`, `refactor`, `test`, `chore`), em pt-br.
6. Abra o PR contra `main` com o gabarito preenchido. Um commit por etapa lógica.

## Regras do código

- Mensagens de wire só em `code/proto/cluster.proto`.
- Sem `unwrap` em caminho de produção; erros com contexto.
- Segredos (tokens, chaves de release) nunca entram no repositório; a chave privada de release fica em `~/.aic`.
- Pesos de modelo não são versionados (`code/models/` é ignorado).

## Segurança

Vulnerabilidade: não use issue pública, veja [`SECURITY.md`](SECURITY.md).

Ao contribuir, você concorda em licenciar sua contribuição sob a [Apache-2.0](LICENSE). Conduta: [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).
